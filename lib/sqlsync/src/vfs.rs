use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{File, FilePtr, OpenKind, Vfs, VfsResult};

use crate::physical::Storage;

pub struct StorageVfs {
    storage: FilePtr<Storage>,
}

impl StorageVfs {
    pub fn new(storage: FilePtr<Storage>) -> Self {
        Self { storage }
    }
}

impl Vfs for StorageVfs {
    type File = FilePtr<Storage>;

    fn open(
        &mut self,
        path: &std::ffi::CStr,
        opts: sqlite_vfs::OpenOptions,
    ) -> VfsResult<Self::File> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("open {} {:?}", path, opts);
        assert!(opts.kind == OpenKind::MainDb);
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
}
