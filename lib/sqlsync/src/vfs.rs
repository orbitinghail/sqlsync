use std::{cell::RefCell, collections::BTreeSet, fmt::Debug, rc::Rc};

use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{OpenKind, Vfs, VfsResult};

#[derive(Debug, PartialEq, Eq)]
pub enum FileKind {
    Main,
    Wal,
    MainJournal,
}

pub trait VfsStorage {
    fn file_size(&self, kind: &FileKind) -> VfsResult<usize>;
    fn truncate(&mut self, kind: &FileKind, size: usize) -> VfsResult<()>;
    fn write(&mut self, kind: &FileKind, offset: usize, buf: &[u8]) -> VfsResult<usize>;
    fn read(&self, kind: &FileKind, offset: usize, buf: &mut [u8]) -> VfsResult<usize>;
    fn shm_map(&mut self, region: usize, size: usize, create: bool) -> VfsResult<*const u8>;
    fn shm_unmap(&mut self) -> VfsResult<()>;
}

pub struct VirtualFile<S: VfsStorage> {
    storage: Rc<RefCell<S>>,
    kind: FileKind,
}

impl<S: VfsStorage> VirtualFile<S> {
    fn new(storage: Rc<RefCell<S>>, kind: FileKind) -> Self {
        Self { storage, kind }
    }
}

impl<S: VfsStorage> Debug for VirtualFile<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualFile")
            .field("kind", &self.kind)
            .finish()
    }
}

impl<S: VfsStorage> sqlite_vfs::File for VirtualFile<S> {
    fn file_size(&self) -> VfsResult<u64> {
        trace!("file_size {:?}", self);
        self.storage
            .borrow()
            .file_size(&self.kind)
            .map(|x| x as u64)
    }

    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        trace!("truncate {:?} {}", self, size);
        self.storage
            .borrow_mut()
            .truncate(&self.kind, size as usize)
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> VfsResult<usize> {
        trace!("write {:?} {} {}", self, pos, buf.len());
        self.storage
            .borrow_mut()
            .write(&self.kind, pos as usize, buf)
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> VfsResult<usize> {
        trace!("read {:?} {} {}", self, pos, buf.len());
        self.storage.borrow().read(&self.kind, pos as usize, buf)
    }

    fn sync(&mut self) -> VfsResult<()> {
        trace!("sync {:?}", self);
        // TODO: implement sync when Storage is backed by durable media
        Ok(())
    }

    fn shm_map(&mut self, region: usize, size: usize, create: bool) -> VfsResult<*const u8> {
        debug!("shm_map {:?} region={} size={}", self, region, size);
        self.storage.borrow_mut().shm_map(region, size, create)
    }

    fn shm_unmap(&mut self) -> VfsResult<()> {
        debug!("shm_unmap {:?}", self);
        self.storage.borrow_mut().shm_unmap()
    }
}

pub struct VirtualVfs<S: VfsStorage> {
    storage: Rc<RefCell<S>>,
    journal: Vec<u8>,
}

impl<S: VfsStorage> VirtualVfs<S> {
    pub fn new(storage: Rc<RefCell<S>>) -> Self {
        Self {
            storage,
            journal: Vec::new(),
        }
    }
}

impl<S: VfsStorage> Vfs for VirtualVfs<S> {
    type File = VirtualFile<S>;

    fn open(
        &mut self,
        path: &std::ffi::CStr,
        opts: sqlite_vfs::OpenOptions,
    ) -> VfsResult<Self::File> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("open {} {:?}", path, opts);

        // TODO: handle opts.access

        Ok(VirtualFile::new(
            self.storage.clone(),
            match opts.kind {
                OpenKind::MainDb => FileKind::Main,
                OpenKind::Wal => FileKind::Wal,
                OpenKind::MainJournal => FileKind::MainJournal,
                _ => panic!("unsupported file kind {:?}", opts.kind),
            },
        ))
    }

    fn delete(&mut self, path: &std::ffi::CStr) -> VfsResult<()> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("delete {}", path);

        // no-op for now
        match path {
            "main.db" => return Err(SQLITE_IOERR),
            "main.db-wal" => self.storage.borrow_mut().truncate(&FileKind::Wal, 0)?,
            "main.db-journal" => self
                .storage
                .borrow_mut()
                .truncate(&FileKind::MainJournal, 0)?,
            _ => return Err(SQLITE_IOERR),
        }

        Ok(())
    }

    fn exists(&mut self, path: &std::ffi::CStr) -> VfsResult<bool> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        trace!("exists {}", path);

        // can we hardcode this?
        Ok(match path {
            "main.db" => self.storage.borrow().file_size(&FileKind::Main)? > 0,
            "main.db-wal" => self.storage.borrow().file_size(&FileKind::Wal)? > 0,
            "main.db-journal" => self.storage.borrow().file_size(&FileKind::MainJournal)? > 0,
            _ => false,
        })
    }
}
