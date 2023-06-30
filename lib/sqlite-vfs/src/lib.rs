#![allow(clippy::question_mark)]
//! Create a custom SQLite virtual file system by implementing the [Vfs] trait and registering it
//! using [register].

use std::cell::Cell;
use std::ffi::{c_void, CStr, CString};
use std::mem::{size_of, ManuallyDrop, MaybeUninit};
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::rc::Rc;
use std::slice;
use std::thread;
use std::time::Duration;
use std::time::Instant;

pub use libsqlite3_sys as ffi;

pub type VfsError = i32;
pub type VfsResult<T> = std::result::Result<T, VfsError>;

// re-export constants that a vfs might want to use, for convenience
pub use ffi::{SQLITE_IOERR, SQLITE_OK};

pub trait ShMem {}

/// A file opened by [Vfs].
///
/// See https://sqlite.org/c3ref/io_methods.html
///
/// Close () will result in a call to drop
pub trait File {
    /// int (*xFileSize)(sqlite3_file*, sqlite3_int64 *pSize);
    fn file_size(&self) -> VfsResult<u64>;

    /// int (*xTruncate)(sqlite3_file*, sqlite3_int64 size);
    fn truncate(&mut self, size: u64) -> VfsResult<()>;

    /// int (*xWrite)(sqlite3_file*, const void*, int iAmt, sqlite3_int64 iOfst);
    fn write(&mut self, pos: u64, buf: &[u8]) -> VfsResult<usize>;

    /// int (*xRead)(sqlite3_file*, void*, int iAmt, sqlite3_int64 iOfst);
    fn read(&mut self, pos: u64, buf: &mut [u8]) -> VfsResult<usize>;

    /// int (*xSync)(sqlite3_file*, int flags);
    fn sync(&mut self) -> VfsResult<()>;

    /// The xSectorSize() method returns the sector size of the device that underlies the file.
    /// The sector size is the minimum write that can be performed without disturbing other bytes in the file.
    ///
    /// int (*xSectorSize)(sqlite3_file*);
    fn sector_size(&self) -> usize {
        1024
    }

    /// The xDeviceCharacteristics() method returns a bit vector describing behaviors of the underlying device:
    ///
    /// int (*xDeviceCharacteristics)(sqlite3_file*);
    fn device_characteristics(&self) -> i32 {
        // For now, simply copied from [memfs] without putting in a lot of thought.
        // [memfs]: (https://github.com/sqlite/sqlite/blob/a959bf53110bfada67a3a52187acd57aa2f34e19/ext/misc/memvfs.c#L271-L276)

        // writes of any size are atomic
        ffi::SQLITE_IOCAP_ATOMIC |
        // after reboot following a crash or power loss, the only bytes in a file that were written
        // at the application level might have changed and that adjacent bytes, even bytes within
        // the same sector are guaranteed to be unchanged
        ffi::SQLITE_IOCAP_POWERSAFE_OVERWRITE |
        // when data is appended to a file, the data is appended first then the size of the file is
        // extended, never the other way around
        ffi::SQLITE_IOCAP_SAFE_APPEND |
        // information is written to disk in the same order as calls to xWrite()
        ffi::SQLITE_IOCAP_SEQUENTIAL
    }
}

/// Allow boxing files, so you can easily return different optimized impls depending on OpenKind
///
/// E.g. you might want to return different impls for journals and main files
impl File for Box<dyn File> {
    fn sector_size(&self) -> usize {
        self.as_ref().sector_size()
    }

    fn device_characteristics(&self) -> i32 {
        self.as_ref().device_characteristics()
    }

    fn file_size(&self) -> VfsResult<u64> {
        self.as_ref().file_size()
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.as_mut().truncate(size)
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> VfsResult<usize> {
        self.as_mut().write(pos, buf)
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.as_mut().read(pos, buf)
    }

    fn sync(&mut self) -> VfsResult<()> {
        self.as_mut().sync()
    }
}

/// Allow File to be an unsafe pointer
pub struct FilePtr<T: File>(*mut T);

impl<T: File> FilePtr<T> {
    pub fn new(f: &mut Box<T>) -> Self {
        Self(&mut **f)
    }
}

impl<T: File> Clone for FilePtr<T> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<T: File> File for FilePtr<T> {
    fn sector_size(&self) -> usize {
        unsafe { (*self.0).sector_size() }
    }

    fn device_characteristics(&self) -> i32 {
        unsafe { (*self.0).device_characteristics() }
    }

    fn file_size(&self) -> VfsResult<u64> {
        unsafe { (*self.0).file_size() }
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        unsafe { (*self.0).truncate(size) }
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> VfsResult<usize> {
        unsafe { (*self.0).write(pos, buf) }
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> VfsResult<usize> {
        unsafe { (*self.0).read(pos, buf) }
    }

    fn sync(&mut self) -> VfsResult<()> {
        unsafe { (*self.0).sync() }
    }
}

/// A sqlite vfs
///
/// See https://sqlite.org/c3ref/vfs.html
pub trait Vfs {
    /// The file returned by [Vfs::open].
    type File: File;

    /// Open the database object (of type `opts.kind`) at `path`.
    ///
    /// int (*xOpen)(sqlite3_vfs*, const char *zName, sqlite3_file*, int flags, int *pOutFlags);
    fn open(&mut self, path: &CStr, opts: OpenOptions) -> VfsResult<Self::File>;

    /// Delete the database object at `path`.
    ///
    /// int (*xDelete)(sqlite3_vfs*, const char *zName, int syncDir);
    fn delete(&mut self, path: &CStr) -> VfsResult<()>;

    /// Check if an object at `path` already exists. This is called from xAccess.
    ///
    /// int (*xAccess)(sqlite3_vfs*, const char *zName, int flags, int *pResOut);
    fn exists(&mut self, path: &CStr) -> VfsResult<bool>;

    /// Check access to `path`. The default implementation always returns `true`.
    ///
    /// int (*xAccess)(sqlite3_vfs*, const char *zName, int flags, int *pResOut);
    #[allow(unused_variables)]
    fn access(&mut self, path: &CStr, write: bool) -> VfsResult<bool> {
        Ok(true)
    }

    /// Generate up to bytes.len() bytes of randomness
    ///
    /// int (*xRandomness)(sqlite3_vfs*, int nByte, char *zOut);
    fn randomness(&self, bytes: &mut [i8]) -> usize {
        use rand::Rng;
        rand::thread_rng().fill(bytes);
        bytes.len()
    }

    /// The xSleep() method causes the calling thread to sleep for at least the number of microseconds given.
    ///
    /// return the number of microseconds that were actually slept
    ///
    /// int (*xSleep)(sqlite3_vfs*, int microseconds);
    fn sleep(&self, n_micro: usize) -> usize {
        let instant = Instant::now();
        thread::sleep(Duration::from_micros(n_micro as u64));
        instant.elapsed().as_micros() as usize
    }

    /// The xCurrentTime() method returns a Julian Day Number for the current date and time as a floating point value.
    ///
    /// int (*xCurrentTime)(sqlite3_vfs*, double*);
    fn current_time(&self) -> f64 {
        let now = time::OffsetDateTime::now_utc().unix_timestamp() as f64;
        2440587.5 + now / 864.0e5
    }

    /// int (*xCurrentTimeInt64)(sqlite3_vfs*, sqlite3_int64*);
    fn current_time_int64(&self) -> i64 {
        let now = time::OffsetDateTime::now_utc().unix_timestamp() as f64;
        ((2440587.5 + now / 864.0e5) * 864.0e5) as i64
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenOptions {
    /// The object type that is being opened.
    pub kind: OpenKind,

    /// The access an object is opened with.
    pub access: OpenAccess,

    /// The file should be deleted when it is closed.
    pub delete_on_close: bool,
}

/// The object type that is being opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenKind {
    MainDb,
    MainJournal,
    TempDb,
    TempJournal,
    TransientDb,
    SubJournal,
    SuperJournal,
    Wal,
}

/// The access an object is opened with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenAccess {
    /// Read access.
    Read,

    /// Write access (includes read access).
    Write,

    /// Create the file if it does not exist (includes write and read access).
    Create,

    /// Create the file, but throw if it it already exist (includes write and read access).
    CreateNew,
}

struct State<V> {
    vfs: V,
    io_methods: ffi::sqlite3_io_methods,
    last_error: Rc<Cell<Option<VfsError>>>,
}

/// Register a virtual file system ([Vfs]) to SQLite.
pub fn register<F: File, V: Vfs<File = F>>(name: &str, vfs: V) -> Result<(), RegisterError> {
    let name = ManuallyDrop::new(CString::new(name)?);
    let io_methods = ffi::sqlite3_io_methods {
        iVersion: 3,
        xClose: Some(io::close::<F>),
        xRead: Some(io::read::<F>),
        xWrite: Some(io::write::<F>),
        xTruncate: Some(io::truncate::<F>),
        xSync: Some(io::sync::<F>),
        xFileSize: Some(io::file_size::<F>),
        xLock: Some(io::lock::<F>),
        xUnlock: Some(io::unlock::<F>),
        xCheckReservedLock: Some(io::check_reserved_lock::<F>),
        xFileControl: Some(io::file_control::<F>),
        xSectorSize: Some(io::sector_size::<F>),
        xDeviceCharacteristics: Some(io::device_characteristics::<F>),
        xShmMap: Some(io::shm_map::<F>),
        xShmLock: Some(io::shm_lock::<F>),
        xShmBarrier: Some(io::shm_barrier),
        xShmUnmap: Some(io::shm_unmap::<F>),
        xFetch: Some(io::mem_fetch::<F>),
        xUnfetch: Some(io::mem_unfetch::<F>),
    };

    let ptr = Box::into_raw(Box::new(State {
        vfs,
        io_methods,
        last_error: Default::default(),
    }));
    let vfs = Box::into_raw(Box::new(ffi::sqlite3_vfs {
        iVersion: 3,
        szOsFile: size_of::<FileState<F>>() as i32,
        mxPathname: MAX_PATH_LENGTH as i32, // max path length supported by VFS
        pNext: null_mut(),
        zName: name.as_ptr(),
        pAppData: ptr as _,
        xOpen: Some(vfs::open::<F, V>),
        xDelete: Some(vfs::delete::<V>),
        xAccess: Some(vfs::access::<V>),
        xFullPathname: Some(vfs::full_pathname::<V>),
        xDlOpen: Some(vfs::dlopen),
        xDlError: Some(vfs::dlerror),
        xDlSym: Some(vfs::dlsym),
        xDlClose: Some(vfs::dlclose),
        xRandomness: Some(vfs::randomness::<V>),
        xSleep: Some(vfs::sleep::<V>),
        xCurrentTime: Some(vfs::current_time::<V>),
        xGetLastError: Some(vfs::get_last_error::<V>),
        xCurrentTimeInt64: Some(vfs::current_time_int64::<V>),
        xSetSystemCall: None,
        xGetSystemCall: None,
        xNextSystemCall: None,
    }));

    let result = unsafe { ffi::sqlite3_vfs_register(vfs, false as i32) };
    if result != ffi::SQLITE_OK {
        return Err(RegisterError::Register(result));
    }

    // TODO: return object that allows to unregister (and cleanup the memory)?

    Ok(())
}

// TODO: add to [Vfs]?
const MAX_PATH_LENGTH: usize = 512;

#[repr(C)]
struct FileState<F> {
    base: ffi::sqlite3_file,
    ext: MaybeUninit<FileExt<F>>,
}

#[repr(C)]
struct FileExt<F> {
    name: String,
    file: F,
    last_error: Rc<Cell<Option<VfsError>>>,
}

// Example mem-fs implementation:
// https://github.com/sqlite/sqlite/blob/a959bf53110bfada67a3a52187acd57aa2f34e19/ext/misc/memvfs.c
mod vfs {
    use super::*;

    /// Open a new file handler.
    pub unsafe extern "C" fn open<F: File, V: Vfs<File = F>>(
        p_vfs: *mut ffi::sqlite3_vfs,
        z_name: *const c_char,
        p_file: *mut ffi::sqlite3_file,
        flags: c_int,
        _p_out_flags: *mut c_int,
    ) -> c_int {
        if z_name.is_null() {
            return ffi::SQLITE_CANTOPEN;
        }
        let name = CStr::from_ptr(z_name).to_string_lossy();
        log::trace!("open z_name={:?} flags={}", name, flags);

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.last_error.take();

        let path = CStr::from_ptr(z_name);

        let opts = match OpenOptions::from_flags(flags) {
            Some(opts) => opts,
            None => {
                state.last_error.set(Some(ffi::SQLITE_IOERR));
                return ffi::SQLITE_CANTOPEN;
            }
        };

        if let Err(err) = state.vfs.open(path, opts).and_then(|file| {
            let out_file = (p_file as *mut FileState<F>)
                .as_mut()
                .ok_or_else(null_ptr_error)?;
            out_file.base.pMethods = &state.io_methods;
            out_file.ext.write(FileExt {
                name: name.to_string(),
                file,
                last_error: Rc::clone(&state.last_error),
            });
            Ok(())
        }) {
            state.last_error.set(Some(err));
            return ffi::SQLITE_CANTOPEN;
        }

        ffi::SQLITE_OK
    }

    /// Delete the file located at `z_path`. If the `sync_dir` argument is true, ensure the
    /// file-system modifications are synced to disk before returning.
    pub unsafe extern "C" fn delete<V: Vfs>(
        p_vfs: *mut ffi::sqlite3_vfs,
        z_path: *const c_char,
        _sync_dir: c_int,
    ) -> c_int {
        let name = if z_path.is_null() {
            None
        } else {
            CStr::from_ptr(z_path).to_str().ok()
        };
        log::trace!("delete z_name={:?}", name);

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_DELETE,
        };
        state.last_error.take();

        let path = CStr::from_ptr(z_path);

        match state.vfs.delete(path.as_ref()) {
            Ok(_) => ffi::SQLITE_OK,
            Err(err) => err,
        }
    }

    /// Test for access permissions. Return true if the requested permission is available, or false
    /// otherwise.
    pub unsafe extern "C" fn access<V: Vfs>(
        p_vfs: *mut ffi::sqlite3_vfs,
        z_path: *const c_char,
        flags: c_int,
        p_res_out: *mut c_int,
    ) -> c_int {
        let name = if z_path.is_null() {
            None
        } else {
            CStr::from_ptr(z_path).to_str().ok()
        };
        log::trace!("access z_name={:?} flags={}", name, flags);

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.last_error.take();

        let path = CStr::from_ptr(z_path);

        let result = match flags {
            ffi::SQLITE_ACCESS_EXISTS => state.vfs.exists(path.as_ref()),
            ffi::SQLITE_ACCESS_READ => state.vfs.access(path.as_ref(), false),
            ffi::SQLITE_ACCESS_READWRITE => state.vfs.access(path.as_ref(), true),
            _ => return ffi::SQLITE_IOERR_ACCESS,
        };

        if let Err(err) = result.and_then(|ok| {
            let p_res_out: &mut c_int = p_res_out.as_mut().ok_or_else(null_ptr_error)?;
            *p_res_out = ok as i32;
            Ok(())
        }) {
            state.last_error.set(Some(err));
            return ffi::SQLITE_IOERR_ACCESS;
        }

        ffi::SQLITE_OK
    }

    /// Populate buffer `z_out` with the full canonical pathname corresponding to the pathname in
    /// `z_path`. `z_out` is guaranteed to point to a buffer of at least (INST_MAX_PATHNAME+1)
    /// bytes.
    pub unsafe extern "C" fn full_pathname<V>(
        p_vfs: *mut ffi::sqlite3_vfs,
        z_path: *const c_char,
        n_out: c_int,
        z_out: *mut c_char,
    ) -> c_int {
        let name = CStr::from_ptr(z_path);
        log::trace!("full_pathname name={}", name.to_string_lossy());

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.last_error.take();

        let name = name.to_bytes_with_nul();
        if name.len() > n_out as usize || name.len() > MAX_PATH_LENGTH {
            return ffi::SQLITE_ERROR;
        }
        let out = slice::from_raw_parts_mut(z_out as *mut u8, name.len());
        out.copy_from_slice(name);

        ffi::SQLITE_OK
    }

    /// Open the dynamic library located at `z_path` and return a handle.
    pub unsafe extern "C" fn dlopen(
        _p_vfs: *mut ffi::sqlite3_vfs,
        _z_path: *const c_char,
    ) -> *mut c_void {
        log::trace!("dlopen");

        null_mut()
    }

    /// Populate the buffer `z_err_msg` (size `n_byte` bytes) with a human readable utf-8 string
    /// describing the most recent error encountered associated with dynamic libraries.
    pub unsafe extern "C" fn dlerror(
        _p_vfs: *mut ffi::sqlite3_vfs,
        n_byte: c_int,
        z_err_msg: *mut c_char,
    ) {
        log::trace!("dlerror");

        let msg = concat!("Loadable extensions are not supported", "\0");
        ffi::sqlite3_snprintf(n_byte, z_err_msg, msg.as_ptr() as _);
    }

    /// Return a pointer to the symbol `z_sym` in the dynamic library pHandle.
    pub unsafe extern "C" fn dlsym(
        _p_vfs: *mut ffi::sqlite3_vfs,
        _p: *mut c_void,
        _z_sym: *const c_char,
    ) -> Option<unsafe extern "C" fn(*mut ffi::sqlite3_vfs, *mut c_void, *const c_char)> {
        log::trace!("dlsym");

        None
    }

    /// Close the dynamic library handle `p_handle`.
    pub unsafe extern "C" fn dlclose(_p_vfs: *mut ffi::sqlite3_vfs, _p_handle: *mut c_void) {
        log::trace!("dlclose");
    }

    /// Populate the buffer pointed to by `z_buf_out` with `n_byte` bytes of random data.
    pub unsafe extern "C" fn randomness<V: Vfs>(
        p_vfs: *mut ffi::sqlite3_vfs,
        n_byte: c_int,
        z_buf_out: *mut c_char,
    ) -> c_int {
        log::trace!("randomness");

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_DELETE,
        };
        state.last_error.take();
        let bytes = slice::from_raw_parts_mut(z_buf_out, n_byte as usize);

        let len = state.vfs.randomness(bytes);
        len as c_int
    }

    /// Sleep for `n_micro` microseconds. Return the number of microseconds actually slept.
    pub unsafe extern "C" fn sleep<V: Vfs>(p_vfs: *mut ffi::sqlite3_vfs, n_micro: c_int) -> c_int {
        log::trace!("sleep");

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_DELETE,
        };
        state.last_error.take();

        let elapsed_us: usize = state.vfs.sleep(n_micro as usize);
        elapsed_us as c_int
    }

    /// Return the current time as a Julian Day number in `p_time_out`.
    pub unsafe extern "C" fn current_time<V: Vfs>(
        p_vfs: *mut ffi::sqlite3_vfs,
        p_time_out: *mut f64,
    ) -> c_int {
        log::trace!("current_time");

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.last_error.take();

        *p_time_out = state.vfs.current_time();
        ffi::SQLITE_OK
    }

    pub unsafe extern "C" fn get_last_error<V>(
        p_vfs: *mut ffi::sqlite3_vfs,
        n_byte: c_int,
        z_err_msg: *mut c_char,
    ) -> c_int {
        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        if let Some(err) = state.last_error.take() {
            let msg = match CString::new(err.to_string()) {
                Ok(msg) => msg,
                Err(_) => return ffi::SQLITE_ERROR,
            };

            let msg = msg.to_bytes_with_nul();
            if msg.len() > n_byte as usize {
                return ffi::SQLITE_ERROR;
            }
            let out = slice::from_raw_parts_mut(z_err_msg as *mut u8, msg.len());
            out.copy_from_slice(msg);
        }
        ffi::SQLITE_OK
    }

    pub unsafe extern "C" fn current_time_int64<V: Vfs>(
        p_vfs: *mut ffi::sqlite3_vfs,
        p: *mut i64,
    ) -> i32 {
        log::trace!("current_time_int64");

        let state = match vfs_state::<V>(p_vfs) {
            Ok(state) => state,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.last_error.take();

        *p = state.vfs.current_time_int64();
        ffi::SQLITE_OK
    }
}

mod io {
    use std::mem;

    use super::*;

    /// Close a file.
    pub unsafe extern "C" fn close<F>(p_file: *mut ffi::sqlite3_file) -> c_int {
        log::trace!("close");

        if let Some(f) = (p_file as *mut FileState<F>).as_mut() {
            let ext = mem::replace(&mut f.ext, MaybeUninit::uninit());
            let mut ext = ext.assume_init(); // extract the value to drop it
            ext.unset_last_error();
        }

        ffi::SQLITE_OK
    }

    /// Read data from a file.
    pub unsafe extern "C" fn read<F: File>(
        p_file: *mut ffi::sqlite3_file,
        z_buf: *mut c_void,
        i_amt: c_int,
        i_ofst: ffi::sqlite3_int64,
    ) -> c_int {
        log::trace!("read offset={} len={}", i_ofst, i_amt);

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_CLOSE,
        };

        let out = slice::from_raw_parts_mut(z_buf as *mut u8, i_amt as usize);
        if let Err(err) = state.file.read(i_ofst as u64, out) {
            return err;
        }

        ffi::SQLITE_OK
    }

    /// Write data to a file.
    pub unsafe extern "C" fn write<F: File>(
        p_file: *mut ffi::sqlite3_file,
        z: *const c_void,
        i_amt: c_int,
        i_ofst: ffi::sqlite3_int64,
    ) -> c_int {
        log::trace!("write offset={} len={}", i_ofst, i_amt);

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_WRITE,
        };

        let data = slice::from_raw_parts(z as *mut u8, i_amt as usize);
        if let Err(err) = state.file.write(i_ofst as u64, data) {
            state.set_last_error(err);
            return ffi::SQLITE_IOERR_WRITE;
        }

        ffi::SQLITE_OK
    }

    /// Truncate a file.
    pub unsafe extern "C" fn truncate<F: File>(
        p_file: *mut ffi::sqlite3_file,
        size: ffi::sqlite3_int64,
    ) -> c_int {
        log::trace!("truncate");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_FSYNC,
        };
        log::trace!("truncate ({})", state.name);

        if let Err(err) = state.file.truncate(size as u64) {
            state.set_last_error(err);
            return ffi::SQLITE_IOERR_TRUNCATE;
        }

        ffi::SQLITE_OK
    }

    /// Persist changes to a file.
    pub unsafe extern "C" fn sync<F: File>(p_file: *mut ffi::sqlite3_file, _flags: c_int) -> c_int {
        log::trace!("sync");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_FSYNC,
        };
        log::trace!("sync ({})", state.name);

        if let Err(err) = state.file.sync() {
            state.set_last_error(err);
            return ffi::SQLITE_IOERR_FSYNC;
        }

        ffi::SQLITE_OK
    }

    /// Return the current file-size of a file.
    pub unsafe extern "C" fn file_size<F: File>(
        p_file: *mut ffi::sqlite3_file,
        p_size: *mut ffi::sqlite3_int64,
    ) -> c_int {
        log::trace!("file_size");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_FSTAT,
        };
        log::trace!("file_size ({})", state.name);

        if let Err(err) = state.file.file_size().and_then(|n| {
            let p_size: &mut ffi::sqlite3_int64 = p_size.as_mut().ok_or_else(null_ptr_error)?;
            *p_size = n as ffi::sqlite3_int64;
            Ok(())
        }) {
            state.set_last_error(err);
            return ffi::SQLITE_IOERR_FSTAT;
        }

        ffi::SQLITE_OK
    }

    /// Lock a file.
    pub unsafe extern "C" fn lock<F>(p_file: *mut ffi::sqlite3_file, _e_lock: c_int) -> c_int {
        log::trace!("lock");

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_IOERR_LOCK;
        }

        // TODO: implement locking
        ffi::SQLITE_OK
    }

    /// Unlock a file.
    pub unsafe extern "C" fn unlock<F>(p_file: *mut ffi::sqlite3_file, _e_lock: c_int) -> c_int {
        log::trace!("unlock");

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_IOERR_UNLOCK;
        }

        // TODO: implement locking
        ffi::SQLITE_OK
    }

    /// Check if another file-handle holds a RESERVED lock on a file.
    pub unsafe extern "C" fn check_reserved_lock<F>(
        p_file: *mut ffi::sqlite3_file,
        p_res_out: *mut c_int,
    ) -> c_int {
        log::trace!("check_reserved_lock");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_IOERR_CHECKRESERVEDLOCK,
        };

        match p_res_out.as_mut() {
            Some(p_res_out) => {
                *p_res_out = false as i32;
            }
            None => {
                state.set_last_error(null_ptr_error());
                return ffi::SQLITE_IOERR_CHECKRESERVEDLOCK;
            }
        }

        // TODO: implement locking
        ffi::SQLITE_OK
    }

    /// File control method. For custom operations on an mem-file.
    pub unsafe extern "C" fn file_control<F>(
        p_file: *mut ffi::sqlite3_file,
        op: c_int,
        _p_arg: *mut c_void,
    ) -> c_int {
        log::trace!("file_control op={}", op);

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_ERROR;
        }

        ffi::SQLITE_NOTFOUND
    }

    /// Return the sector-size in bytes for a file.
    pub unsafe extern "C" fn sector_size<F: File>(p_file: *mut ffi::sqlite3_file) -> c_int {
        log::trace!("sector_size");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_ERROR,
        };

        state.file.sector_size() as c_int
    }

    /// Return the device characteristic flags supported by a file.
    pub unsafe extern "C" fn device_characteristics<F: File>(
        p_file: *mut ffi::sqlite3_file,
    ) -> c_int {
        log::trace!("device_characteristics");

        let state = match file_state::<F>(p_file, true) {
            Ok(f) => f,
            Err(_) => return ffi::SQLITE_ERROR,
        };
        state.file.device_characteristics() as c_int
    }

    /// Create a shared memory file mapping.
    pub unsafe extern "C" fn shm_map<F>(
        p_file: *mut ffi::sqlite3_file,
        i_pg: i32,
        pgsz: i32,
        b_extend: i32,
        _pp: *mut *mut c_void,
    ) -> i32 {
        log::trace!("shm_map pg={} sz={} extend={}", i_pg, pgsz, b_extend);

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_IOERR_SHMMAP;
        }

        ffi::SQLITE_IOERR_SHMMAP
    }

    /// Perform locking on a shared-memory segment.
    pub unsafe extern "C" fn shm_lock<F>(
        p_file: *mut ffi::sqlite3_file,
        _offset: i32,
        _n: i32,
        _flags: i32,
    ) -> i32 {
        log::trace!("shm_lock");

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_IOERR_SHMMAP;
        }

        ffi::SQLITE_IOERR_SHMLOCK
    }

    /// Memory barrier operation on shared memory.
    pub unsafe extern "C" fn shm_barrier(_p_file: *mut ffi::sqlite3_file) {
        log::trace!("shm_barrier");
    }

    /// Unmap a shared memory segment.
    pub unsafe extern "C" fn shm_unmap<F>(
        p_file: *mut ffi::sqlite3_file,
        _delete_flags: i32,
    ) -> i32 {
        log::trace!("shm_unmap");

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_IOERR_SHMMAP;
        }

        ffi::SQLITE_OK
    }

    /// Fetch a page of a memory-mapped file.
    pub unsafe extern "C" fn mem_fetch<F: File>(
        p_file: *mut ffi::sqlite3_file,
        i_ofst: i64,
        i_amt: i32,
        _pp: *mut *mut c_void,
    ) -> i32 {
        log::trace!("mem_fetch offset={} len={}", i_ofst, i_amt);

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_ERROR;
        }

        ffi::SQLITE_ERROR
    }

    /// Release a memory-mapped page.
    pub unsafe extern "C" fn mem_unfetch<F>(
        p_file: *mut ffi::sqlite3_file,
        i_ofst: i64,
        _p_page: *mut c_void,
    ) -> i32 {
        log::trace!("mem_unfetch offset={}", i_ofst);

        // reset last error
        if file_state::<F>(p_file, true).is_err() {
            return ffi::SQLITE_ERROR;
        }

        ffi::SQLITE_OK
    }
}

impl<F> FileExt<F> {
    fn unset_last_error(&mut self) {
        self.last_error.take();
    }

    fn set_last_error(&mut self, err: VfsError) {
        self.last_error.set(Some(err));
    }
}

fn null_ptr_error() -> VfsError {
    ffi::SQLITE_IOERR
}

unsafe fn vfs_state<'a, V>(ptr: *mut ffi::sqlite3_vfs) -> VfsResult<&'a mut State<V>> {
    let vfs: &mut ffi::sqlite3_vfs = ptr.as_mut().ok_or_else(null_ptr_error)?;
    let state = (vfs.pAppData as *mut State<V>)
        .as_mut()
        .ok_or_else(null_ptr_error)?;
    Ok(state)
}

unsafe fn file_state<'a, F>(
    ptr: *mut ffi::sqlite3_file,
    reset_last_error: bool,
) -> VfsResult<&'a mut FileExt<F>> {
    let f = (ptr as *mut FileState<F>)
        .as_mut()
        .ok_or_else(null_ptr_error)?;
    let ext = f.ext.assume_init_mut();
    if reset_last_error {
        ext.unset_last_error();
    }
    Ok(ext)
}

impl OpenOptions {
    fn from_flags(flags: i32) -> Option<Self> {
        Some(OpenOptions {
            kind: OpenKind::from_flags(flags)?,
            access: OpenAccess::from_flags(flags)?,
            delete_on_close: flags & ffi::SQLITE_OPEN_DELETEONCLOSE > 0,
        })
    }
}

impl OpenKind {
    fn from_flags(flags: i32) -> Option<Self> {
        match flags {
            flags if flags & ffi::SQLITE_OPEN_MAIN_DB > 0 => Some(Self::MainDb),
            flags if flags & ffi::SQLITE_OPEN_MAIN_JOURNAL > 0 => Some(Self::MainJournal),
            flags if flags & ffi::SQLITE_OPEN_TEMP_DB > 0 => Some(Self::TempDb),
            flags if flags & ffi::SQLITE_OPEN_TEMP_JOURNAL > 0 => Some(Self::TempJournal),
            flags if flags & ffi::SQLITE_OPEN_TRANSIENT_DB > 0 => Some(Self::TransientDb),
            flags if flags & ffi::SQLITE_OPEN_SUBJOURNAL > 0 => Some(Self::SubJournal),
            flags if flags & ffi::SQLITE_OPEN_SUPER_JOURNAL > 0 => Some(Self::SuperJournal),
            flags if flags & ffi::SQLITE_OPEN_WAL > 0 => Some(Self::Wal),
            _ => None,
        }
    }
}

impl OpenAccess {
    fn from_flags(flags: i32) -> Option<Self> {
        match flags {
            flags
                if (flags & ffi::SQLITE_OPEN_CREATE > 0)
                    && (flags & ffi::SQLITE_OPEN_EXCLUSIVE > 0) =>
            {
                Some(Self::CreateNew)
            }
            flags if flags & ffi::SQLITE_OPEN_CREATE > 0 => Some(Self::Create),
            flags if flags & ffi::SQLITE_OPEN_READWRITE > 0 => Some(Self::Write),
            flags if flags & ffi::SQLITE_OPEN_READONLY > 0 => Some(Self::Read),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum RegisterError {
    Nul(std::ffi::NulError),
    Register(i32),
}

impl std::error::Error for RegisterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Nul(err) => Some(err),
            Self::Register(_) => None,
        }
    }
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nul(_) => f.write_str("interior nul byte in name found"),
            Self::Register(code) => {
                write!(f, "registering sqlite vfs failed with error code: {}", code)
            }
        }
    }
}

impl From<std::ffi::NulError> for RegisterError {
    fn from(err: std::ffi::NulError) -> Self {
        Self::Nul(err)
    }
}
