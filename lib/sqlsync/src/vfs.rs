use std::{cell::RefCell, fmt::Debug, rc::Rc};

use byteorder::BigEndian;
use libsqlite3_sys::SQLITE_IOERR;
use log::{debug, trace};
use sqlite_vfs::{File, OpenKind, Vfs};

use crate::layout::{
    wal_checksum, wal_header, wal_index_header_info, WAL_HEADER_SIZE,
    WAL_INDEX_HEADER_CHECKPOINT_SIZE, WAL_INDEX_HEADER_INFO_SIZE,
};

pub const PAGESIZE: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
enum FileKind {
    Main,
    Wal,
    MainJournal,
}

#[derive(Debug)]
pub struct Storage {
    main: Vec<u8>,
    wal: Vec<u8>,
    journal: Vec<u8>,
    shm: Vec<Vec<u8>>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            main: Vec::new(),
            wal: Vec::new(),
            journal: Vec::new(),
            shm: Vec::new(),
        }
    }

    fn data_mut(&mut self, kind: &FileKind) -> &mut Vec<u8> {
        match kind {
            FileKind::Main => &mut self.main,
            FileKind::Wal => &mut self.wal,
            FileKind::MainJournal => &mut self.journal,
        }
    }

    fn data(&self, kind: &FileKind) -> &Vec<u8> {
        match kind {
            FileKind::Main => &self.main,
            FileKind::Wal => &self.wal,
            FileKind::MainJournal => &self.journal,
        }
    }

    fn shm_map(&mut self, region: usize, size: usize, create: bool) -> *const u8 {
        // we expect all shm allocations to be 16KB
        const EXPECTED_SIZE: usize = 2 << 14;
        assert!(size == EXPECTED_SIZE, "unexpected shm_map size {}", size);

        if self.shm.get(region).is_none() {
            assert!(
                region == self.shm.len(),
                "unexpected shm_map region {}",
                region
            );
            if create {
                self.shm.resize(region + 1, Vec::new());
            } else {
                // create is false, region doesn't exist, return null ptr
                return std::ptr::null();
            }
        }

        let data = &mut self.shm[region];
        if data.is_empty() {
            data.resize(size, 0);
        }
        assert!(data.len() == size, "unexpected shm_map size {}", data.len());

        data.as_ptr()
    }

    fn shm_unmap(&mut self) {
        self.shm.clear();
    }

    fn db_size_in_pages(&self) -> u32 {
        (self.main.len() / PAGESIZE) as u32
    }

    fn wal_header_checksum(&self) -> (u32, u32) {
        let wal_hdr = wal_header::View::new(&self.wal);
        (wal_hdr.checksum1().read(), wal_hdr.checksum2().read())
    }

    fn wal_header_salts(&self) -> (u32, u32) {
        let wal_hdr = wal_header::View::new(&self.wal);
        (wal_hdr.salt1().read(), wal_hdr.salt2().read())
    }

    pub fn rollback(&mut self) {
        self.rollback_wal();
        self.reset_shm();
    }

    fn rollback_wal(&mut self) {
        // read previous wal's salt1
        let prev_salt1 = wal_header::View::new(&self.wal).salt1().read();

        // create a new empty wal header
        let mut wal_hdr = wal_header::View::new([0u8; WAL_HEADER_SIZE]);

        // 0x377f0682 == BigEndian
        wal_hdr.magic_mut().write(0x377f0683);
        wal_hdr.file_format_write_version_mut().write(3007000);
        wal_hdr.page_size_mut().write(PAGESIZE as u32);
        wal_hdr.checkpoint_sequence_number_mut().write(0);
        wal_hdr.salt1_mut().write(prev_salt1.wrapping_add(1));
        wal_hdr.salt2_mut().write(rand::random::<u32>());

        // calculate and store the wal checksum
        let wal_hdr = wal_hdr.into_storage();
        let (checksum1, checksum2) = wal_checksum::<BigEndian>(0, 0, &wal_hdr[0..24]);
        let mut wal_hdr = wal_header::View::new(wal_hdr);
        wal_hdr.checksum1_mut().write(checksum1);
        wal_hdr.checksum2_mut().write(checksum2);

        // truncate the wal to the new header length
        self.wal.truncate(WAL_HEADER_SIZE);

        // write the new header
        self.wal.copy_from_slice(&wal_hdr.into_storage());
    }

    fn reset_shm(&mut self) {
        // TODO: need to test this when the shm has more than one block

        let next_ichange = match self.shm.get(0) {
            Some(shm) => {
                let hdr = wal_index_header_info::View::new(shm);
                hdr.ichange().read().wrapping_add(1)
            }
            None => 0,
        };
        let (wal_chksum1, wal_chksum2) = self.wal_header_checksum();
        let (wal_salt1, wal_salt2) = self.wal_header_salts();

        // create a new shm header info block
        let mut hdr = wal_index_header_info::View::new([0u8; WAL_INDEX_HEADER_INFO_SIZE]);

        hdr.iversion_mut().write(3007000);
        hdr.ichange_mut().write(next_ichange);
        hdr.is_init_mut().write(1);
        hdr.big_endian_checksum_mut().write(1);
        hdr.page_size_mut().write(PAGESIZE as u16);
        hdr.max_frame_count_mut().write(0);
        hdr.database_size_mut().write(self.db_size_in_pages());
        hdr.last_frame_checksum1_mut().write(wal_chksum1);
        hdr.last_frame_checksum2_mut().write(wal_chksum2);
        hdr.salt1_mut().write(wal_salt1);
        hdr.salt2_mut().write(wal_salt2);

        // calculate and store the shm header checksum
        let hdr = hdr.into_storage();
        let (checksum1, checksum2) = wal_checksum::<BigEndian>(0, 0, &hdr[0..40]);
        let mut hdr = wal_index_header_info::View::new(hdr);
        hdr.checksum1_mut().write(checksum1);
        hdr.checksum2_mut().write(checksum2);

        // create a new shm header checkpoint block
        let info_hdr = hdr.into_storage();
        let checkpoint = [0u8; WAL_INDEX_HEADER_CHECKPOINT_SIZE];

        // create and initialize the full shm header
        let mut full_hdr =
            [0u8; (WAL_INDEX_HEADER_INFO_SIZE * 2) + WAL_INDEX_HEADER_CHECKPOINT_SIZE];
        full_hdr[0..WAL_INDEX_HEADER_INFO_SIZE].copy_from_slice(&info_hdr);
        full_hdr[WAL_INDEX_HEADER_INFO_SIZE..WAL_INDEX_HEADER_INFO_SIZE * 2]
            .copy_from_slice(&info_hdr);
        full_hdr[WAL_INDEX_HEADER_INFO_SIZE * 2..].copy_from_slice(&checkpoint);

        if self.shm.is_empty() {
            let mut full_hdr = full_hdr.to_vec();
            full_hdr.resize(2 << 14, 0);
            self.shm.push(full_hdr);
        } else {
            self.shm.truncate(1);
            let shm = &mut self.shm[0];
            shm[0..full_hdr.len()].copy_from_slice(&full_hdr);
            // zero rest of shm
            shm[full_hdr.len()..].fill(0);
        }
    }
}

pub struct VirtualFile {
    storage: Rc<RefCell<Storage>>,
    kind: FileKind,
}

impl VirtualFile {
    fn new(storage: Rc<RefCell<Storage>>, kind: FileKind) -> Self {
        Self { storage, kind }
    }
}

impl Debug for VirtualFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualFile")
            .field("kind", &self.kind)
            .finish()
    }
}

impl sqlite_vfs::File for VirtualFile {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        trace!("file_size {:?}", self);
        match self.kind {
            FileKind::Main => Ok(self.storage.borrow().main.len() as u64),
            FileKind::Wal => Ok(self.storage.borrow().wal.len() as u64),
            FileKind::MainJournal => Ok(self.storage.borrow().journal.len() as u64),
        }
    }

    fn truncate(&mut self, size: u64) -> sqlite_vfs::VfsResult<()> {
        trace!("truncate {:?} {}", self, size);
        let mut storage = self.storage.borrow_mut();
        let data = storage.data_mut(&self.kind);
        data.truncate(size as usize);
        Ok(())
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> sqlite_vfs::VfsResult<usize> {
        trace!("write {:?} {} {}", self, pos, buf.len());

        let mut storage = self.storage.borrow_mut();
        let data = storage.data_mut(&self.kind);

        let current_len = data.len();
        let write_len = buf.len();
        let start = usize::try_from(pos).unwrap();
        let end = start + write_len;

        if start > current_len {
            // write start is out of range
            return Err(SQLITE_IOERR);
        }

        if end > current_len {
            // write end is out of range
            data.resize(end, 0);
        }

        data[start..end].copy_from_slice(buf);

        Ok(write_len)
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> sqlite_vfs::VfsResult<usize> {
        trace!("read {:?} {} {}", self, pos, buf.len());

        let storage = self.storage.borrow();
        let data = storage.data(&self.kind);

        let start = usize::try_from(pos).unwrap();
        let remaining = data.len().saturating_sub(start);
        let n = remaining.min(buf.len());
        if n != 0 {
            buf[..n].copy_from_slice(&data[start..start + n]);
        }

        Ok(n)
    }

    fn sync(&mut self) -> sqlite_vfs::VfsResult<()> {
        trace!("sync {:?}", self);
        // TODO: implement sync when Storage is backed by durable media
        Ok(())
    }

    fn shm_map(
        &mut self,
        region: usize,
        size: usize,
        create: bool,
    ) -> sqlite_vfs::VfsResult<*const u8> {
        debug!("shm_map {:?} region={} size={}", self, region, size);
        Ok(self.storage.borrow_mut().shm_map(region, size, create))
    }

    fn shm_unmap(&mut self) -> sqlite_vfs::VfsResult<()> {
        debug!("shm_unmap {:?}", self);
        Ok(self.storage.borrow_mut().shm_unmap())
    }
}

pub struct VirtualVfs {
    pub storage: Rc<RefCell<Storage>>,
}

impl Vfs for VirtualVfs {
    type File = VirtualFile;

    fn open(
        &mut self,
        path: &std::ffi::CStr,
        opts: sqlite_vfs::OpenOptions,
    ) -> sqlite_vfs::VfsResult<Self::File> {
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

    fn delete(&mut self, path: &std::ffi::CStr) -> sqlite_vfs::VfsResult<()> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        debug!("delete {}", path);

        match path {
            "main.db" => self.storage.borrow_mut().main.clear(),
            "main.db-wal" => self.storage.borrow_mut().wal.clear(),
            "main.db-journal" => self.storage.borrow_mut().journal.clear(),
            _ => return Err(SQLITE_IOERR),
        }

        Ok(())
    }

    fn exists(&mut self, path: &std::ffi::CStr) -> sqlite_vfs::VfsResult<bool> {
        let path = path.to_str().map_err(|_err| SQLITE_IOERR)?;
        trace!("exists {}", path);

        Ok(match path {
            "main.db" => self.storage.borrow().main.len() != 0,
            "main.db-wal" => self.storage.borrow().wal.len() != 0,
            "main.db-journal" => self.storage.borrow().journal.len() != 0,
            _ => false,
        })
    }
}
