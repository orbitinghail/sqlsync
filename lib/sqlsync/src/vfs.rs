use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{File, OpenKind, Vfs, VfsResult};

use crate::physical::Storage;

#[derive(Clone)]
pub struct RcStorage(Rc<RefCell<Storage>>);

impl RcStorage {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Storage::new())))
    }

    pub fn borrow(&self) -> Ref<Storage> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<Storage> {
        self.0.borrow_mut()
    }
}

impl sqlite_vfs::File for RcStorage {
    fn file_size(&self) -> VfsResult<u64> {
        self.borrow().file_size()
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.borrow_mut().truncate(size)
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> VfsResult<usize> {
        self.borrow_mut().write(pos, buf)
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> VfsResult<usize> {
        self.borrow_mut().read(pos, buf)
    }

    fn sync(&mut self) -> VfsResult<()> {
        self.borrow_mut().sync()
    }
}

pub struct StorageVfs {
    storage: RcStorage,
}

impl StorageVfs {
    pub fn new(storage: RcStorage) -> Self {
        Self { storage }
    }
}

impl Vfs for StorageVfs {
    type File = RcStorage;

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
