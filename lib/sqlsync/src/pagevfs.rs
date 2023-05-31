use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    fmt::Debug,
    hash::Hash,
    rc::Rc,
};

use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{OpenAccess, Vfs};

type FileIdx = u16;
type PageIdx = u32;

#[derive(Debug, PartialEq, Eq)]
enum Layer {
    Root,
    Branch,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct PageKey {
    fd: FileIdx,
    page: PageIdx,
}

impl PageKey {
    fn new(fd: FileIdx, page: PageIdx) -> Self {
        Self { fd, page }
    }

    fn root(&self) -> Self {
        Self {
            fd: self.fd,
            page: self.page,
        }
    }
}

type Page<const PAGESIZE: usize> = [u8; PAGESIZE];

#[derive(Debug)]
pub struct Storage<const PAGESIZE: usize> {
    files: BTreeMap<String, FileIdx>,
    root_pages: BTreeMap<PageKey, Page<PAGESIZE>>,
    branch_pages: BTreeMap<PageKey, Page<PAGESIZE>>,
    layer: Layer,
}

impl<const PAGESIZE: usize> Storage<PAGESIZE> {
    pub fn new() -> Self {
        Self {
            files: Default::default(),
            root_pages: Default::default(),
            branch_pages: Default::default(),
            layer: Layer::Root,
        }
    }

    fn file_get(&self, name: &str) -> Option<FileIdx> {
        self.files.get(name).copied()
    }

    fn file_exists(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }

    fn file_get_or_create(&mut self, name: &str) -> FileIdx {
        if let Some(fd) = self.files.get(name) {
            *fd
        } else {
            let fd = self.files.len() as FileIdx;
            self.files.insert(name.to_owned(), fd);
            fd
        }
    }

    fn file_delete(&mut self, name: &str) -> bool {
        self.files.remove(name).is_some()
    }

    fn file_size(&self, fd: FileIdx) -> usize {
        if self.layer == Layer::Root {
            self.root_pages.keys().filter(|k| k.fd == fd).count() * PAGESIZE
        } else {
            let root_keys: HashSet<_> = self.root_pages.keys().filter(|k| k.fd == fd).collect();
            let branch_keys: HashSet<_> = self.branch_pages.keys().filter(|k| k.fd == fd).collect();
            root_keys.union(&branch_keys).count() * PAGESIZE
        }
    }

    fn page_entry<'a>(&'a mut self, fd: FileIdx, page: PageIdx) -> &'a mut Page<PAGESIZE> {
        let key = PageKey::new(fd, page);

        if self.layer == Layer::Root {
            self.root_pages.entry(key).or_insert([0; PAGESIZE])
        } else {
            self.branch_pages.entry(key).or_insert_with(|| {
                self.root_pages
                    .get(&key.root())
                    .copied()
                    .unwrap_or_else(|| [0; PAGESIZE])
            })
        }
    }

    fn page(&self, fd: FileIdx, page: PageIdx) -> Option<&Page<PAGESIZE>> {
        let key = PageKey::new(fd, page);
        if self.layer == Layer::Root {
            self.root_pages.get(&key)
        } else {
            self.branch_pages
                .get(&key)
                .or_else(|| self.root_pages.get(&key))
        }
    }

    pub fn branch(&mut self) {
        self.layer = Layer::Branch;
    }

    // no-op if we are on the root layer
    // move all pages at the current layer to the root
    // transition to the root layer
    pub fn commit(&mut self) {
        self.root_pages.append(&mut self.branch_pages);
        self.layer = Layer::Root;
    }

    // throw away all pages on the Branch layer
    // transition to the root layer
    pub fn rollback(&mut self) {
        self.branch_pages.clear();
        self.layer = Layer::Root;
    }
}

pub struct VirtualFile<const PAGESIZE: usize> {
    storage: Rc<RefCell<Storage<PAGESIZE>>>,
    fd: FileIdx,
}

impl<const PAGESIZE: usize> Debug for VirtualFile<PAGESIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualFile").field("fd", &self.fd).finish()
    }
}

impl<const PAGESIZE: usize> sqlite_vfs::File for VirtualFile<PAGESIZE> {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        trace!("file_size {:?}", self);
        Ok(self.storage.borrow().file_size(self.fd) as u64)
    }

    fn truncate(&mut self, size: u64) -> sqlite_vfs::VfsResult<()> {
        trace!("truncate {:?} {}", self, size);
        panic!("not implemented")
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> sqlite_vfs::VfsResult<usize> {
        trace!("write {:?} {} {}", self, pos, buf.len());
        if buf.len() > PAGESIZE {
            return Err(SQLITE_IOERR);
        }

        // calculate the page offset
        let page = (pos / PAGESIZE as u64) as PageIdx;
        // calculate the position within the page to write
        let offset = (pos % PAGESIZE as u64) as usize;

        if offset + buf.len() > PAGESIZE {
            return Err(SQLITE_IOERR);
        }

        let mut storage = self.storage.borrow_mut();
        let page = storage.page_entry(self.fd, page);
        page[offset..offset + buf.len()].copy_from_slice(buf);

        Ok(buf.len())
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> sqlite_vfs::VfsResult<usize> {
        trace!("read {:?} {} {}", self, pos, buf.len());
        if buf.len() > PAGESIZE {
            return Err(SQLITE_IOERR);
        }

        // calculate the page offset
        let page = (pos / PAGESIZE as u64) as PageIdx;
        // calculate the position within the page to read
        let offset = (pos % PAGESIZE as u64) as usize;

        if offset + buf.len() > PAGESIZE {
            return Err(SQLITE_IOERR);
        }

        let storage = self.storage.borrow();
        let page = storage.page(self.fd, page);
        Ok(match page {
            Some(page) => {
                buf.copy_from_slice(&page[offset..offset + buf.len()]);
                buf.len()
            }
            None => 0,
        })
    }

    fn sync(&mut self) -> sqlite_vfs::VfsResult<()> {
        trace!("sync {:?}", self);
        Ok(())
    }

    fn sector_size(&self) -> usize {
        1024
    }

    fn device_characteristics(&self) -> i32 {
        // writes of any size are atomic
        libsqlite3_sys::SQLITE_IOCAP_ATOMIC |
        // after reboot following a crash or power loss, the only bytes in a file that were written
        // at the application level might have changed and that adjacent bytes, even bytes within
        // the same sector are guaranteed to be unchanged
        libsqlite3_sys::SQLITE_IOCAP_POWERSAFE_OVERWRITE |
        // when data is appended to a file, the data is appended first then the size of the file is
        // extended, never the other way around
        libsqlite3_sys::SQLITE_IOCAP_SAFE_APPEND |
        // information is written to disk in the same order as calls to xWrite()
        libsqlite3_sys::SQLITE_IOCAP_SEQUENTIAL
    }
}

pub struct StorageVfs<const PAGESIZE: usize> {
    pub storage: Rc<RefCell<Storage<PAGESIZE>>>,
}

impl<const PAGESIZE: usize> Vfs for StorageVfs<PAGESIZE> {
    type File = VirtualFile<PAGESIZE>;

    fn open(
        &mut self,
        path: &std::ffi::CStr,
        opts: sqlite_vfs::OpenOptions,
    ) -> sqlite_vfs::VfsResult<Self::File> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("open {} {:?}", path, opts);

        let file = match opts.access {
            OpenAccess::Read => todo!(),
            OpenAccess::Write => {
                let storage = self.storage.borrow();
                let fd = storage.file_get(path).ok_or(SQLITE_IOERR)?;
                VirtualFile {
                    storage: self.storage.clone(),
                    fd,
                }
            }
            OpenAccess::Create => {
                let mut storage = self.storage.borrow_mut();
                let fd = storage.file_get_or_create(path);
                VirtualFile {
                    storage: self.storage.clone(),
                    fd,
                }
            }
            OpenAccess::CreateNew => {
                let mut storage = self.storage.borrow_mut();
                if storage.file_exists(path) {
                    return Err(SQLITE_IOERR);
                }
                let fd = storage.file_get_or_create(path);
                VirtualFile {
                    storage: self.storage.clone(),
                    fd,
                }
            }
        };

        Ok(file)
    }

    fn delete(&mut self, path: &std::ffi::CStr) -> sqlite_vfs::VfsResult<()> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("delete {}", path);

        let mut storage = self.storage.borrow_mut();
        if storage.file_delete(path) {
            Ok(())
        } else {
            Err(SQLITE_IOERR)
        }
    }

    fn exists(&mut self, path: &std::ffi::CStr) -> sqlite_vfs::VfsResult<bool> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        trace!("exists {}", path);

        let storage = self.storage.borrow();
        Ok(storage.file_exists(path))
    }
}
