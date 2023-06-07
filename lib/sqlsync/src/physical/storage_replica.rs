use anyhow::Result;
use libsqlite3_sys::SQLITE_IOERR;

use crate::vfs::{FileKind, VfsStorage};

use super::{
    changeset::Changeset, cursor::Cursor, sqlite_shm::SqliteShm, sqlite_wal::SqliteWal, PAGESIZE,
};

pub struct StorageReplica {
    cursor: Cursor,
    main: Vec<u8>,
    journal: Vec<u8>,
    wal: SqliteWal,
    shm: SqliteShm,
}

impl StorageReplica {
    pub fn new() -> Self {
        Self {
            cursor: Cursor::new(),
            main: Vec::new(),
            journal: Vec::new(),
            wal: SqliteWal::new(),
            shm: SqliteShm::new(),
        }
    }

    fn rollback_local_changes(&mut self) -> Result<()> {
        assert!(
            self.main.len() % PAGESIZE == 0,
            "main db is not page aligned"
        );
        let db_page_count = self.main.len() / PAGESIZE;
        self.wal.reset();
        self.shm.reset(db_page_count, &self.wal);
        Ok(())
    }

    fn fast_forward(&mut self, changes: Changeset) -> Result<()> {
        for (&idx, page) in changes.iter() {
            let offset = (idx as usize) * (PAGESIZE);
            let end = offset + page.len();
            assert!(end <= self.main.len(), "page is out of range");
            self.main[offset..end].copy_from_slice(page);
        }
        Ok(())
    }
}

impl VfsStorage for StorageReplica {
    fn file_size(&self, kind: &FileKind) -> sqlite_vfs::VfsResult<usize> {
        Ok(match kind {
            FileKind::Main => self.main.len(),
            FileKind::Wal => self.wal.len(),
            FileKind::MainJournal => self.journal.len(),
        })
    }

    fn truncate(&mut self, kind: &FileKind, size: usize) -> sqlite_vfs::VfsResult<()> {
        match kind {
            FileKind::Main => self.main.truncate(size),
            FileKind::Wal => self.wal.truncate(size),
            FileKind::MainJournal => self.journal.truncate(size),
        }
        Ok(())
    }

    fn write(
        &mut self,
        kind: &FileKind,
        offset: usize,
        buf: &[u8],
    ) -> sqlite_vfs::VfsResult<usize> {
        if *kind == FileKind::Wal {
            return self.wal.write(offset, buf).map_err(|_| SQLITE_IOERR);
        }

        // TODO: this can be killed once we are bootstrapping main.db from the
        // server as we will no longer need to write main.db or the journal file
        // locally to set things up
        let data = match kind {
            FileKind::Main => &mut self.main,
            FileKind::Wal => unreachable!(),
            FileKind::MainJournal => &mut self.journal,
        };
        let current_len = data.len();
        let write_len = buf.len();
        let end = offset + write_len;

        if offset > current_len {
            // write start is out of range
            return Err(SQLITE_IOERR);
        }

        if end > current_len {
            // write end is out of range
            data.resize(end, 0);
        }

        data[offset..end].copy_from_slice(buf);

        Ok(write_len)
    }

    fn read(&self, kind: &FileKind, offset: usize, buf: &mut [u8]) -> sqlite_vfs::VfsResult<usize> {
        match kind {
            FileKind::Main => {
                let remaining = self.main.len().saturating_sub(offset);
                let n = remaining.min(buf.len());
                if n != 0 {
                    buf[..n].copy_from_slice(&self.main[offset..offset + n]);
                }
                Ok(n)
            }
            FileKind::Wal => self.wal.read(offset, buf).map_err(|_| SQLITE_IOERR),
            FileKind::MainJournal => {
                // TODO: this can be killed once we are bootstrapping main.db from the
                // server as we will no longer need to read the journal file
                // locally to set things up
                let remaining = self.journal.len().saturating_sub(offset);
                let n = remaining.min(buf.len());
                if n != 0 {
                    buf[..n].copy_from_slice(&self.journal[offset..offset + n]);
                }
                Ok(n)
            }
        }
    }

    fn shm_map(
        &mut self,
        region: usize,
        size: usize,
        create: bool,
    ) -> sqlite_vfs::VfsResult<*const u8> {
        Ok(self.shm.shm_map(region, size, create))
    }

    fn shm_unmap(&mut self) -> sqlite_vfs::VfsResult<()> {
        Ok(self.shm.shm_unmap())
    }
}
