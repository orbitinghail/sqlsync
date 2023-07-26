use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{File, FilePtr, OpenKind, Vfs, VfsResult};

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
