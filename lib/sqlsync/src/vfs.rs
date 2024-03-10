use std::pin::Pin;

use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{File, OpenKind, Vfs, VfsResult};

use crate::{journal::Journal, storage::Storage, unixtime::unix_timestamp_milliseconds};

pub struct StorageVfs<J: Journal> {
    storage: FilePtr<Storage<J>>,
}

impl<J: Journal> StorageVfs<J> {
    pub fn new(storage: FilePtr<Storage<J>>) -> Self {
        Self { storage }
    }
}

impl<J: Journal> Vfs for StorageVfs<J> {
    type File = FilePtr<Storage<J>>;

    fn open(
        &mut self,
        path: &std::ffi::CStr,
        opts: sqlite_vfs::OpenOptions,
    ) -> VfsResult<Self::File> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("open {} {:?}", path, opts);
        assert!(
            opts.kind == OpenKind::MainDb,
            "only main.db is supported, got {:?}",
            opts
        );
        Ok(self.storage.clone())
    }

    fn delete(&mut self, path: &std::ffi::CStr) -> VfsResult<()> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("delete {}", path);
        Ok(())
    }

    fn exists(&mut self, path: &std::ffi::CStr) -> VfsResult<bool> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        trace!("exists {}", path);
        Ok(match path {
            "main.db" => self.storage.file_size().unwrap_or(0) > 0,
            _ => false,
        })
    }

    /// The xCurrentTime() method returns a Julian Day Number for the current date and time as a floating point value.
    fn current_time(&self) -> f64 {
        let now = unix_timestamp_milliseconds() as f64;
        2440587.5 + now / 864.0e5
    }

    /// The xCurrentTime() method returns a Julian Day Number for the current date and time as a floating point value.
    fn current_time_int64(&self) -> i64 {
        let now = unix_timestamp_milliseconds() as f64;
        ((2440587.5 + now / 864.0e5) * 864.0e5) as i64
    }
}

/// Allow File to be an unsafe pointer
pub struct FilePtr<T: File>(*mut T);

impl<T: File> FilePtr<T> {
    pub fn new(f: &mut Pin<Box<T>>) -> Self {
        // SAFETY: we are creating a raw pointer from a reference to a Box
        // and we are not moving the Box, so the Box will not be dropped
        // while the raw pointer is still in use.
        Self(unsafe { f.as_mut().get_unchecked_mut() })
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
