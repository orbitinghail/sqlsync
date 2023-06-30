use std::fmt::Debug;

use super::{page::SparsePages, PAGESIZE};
use crate::{
    journal::{Cursor, Journal, JournalPartial},
    physical::page::Page,
};

// TODO: eventually we should decide on MAX_SYNC based on the number of pages
// per journal entry
// idea: push this down to Journal via some kind of "include" callback to decide
// which entries to include in order (once it returns false, return the partial)
const MAX_SYNC: usize = 1;

pub struct Storage {
    journal: Journal<SparsePages>,
    pending: SparsePages,
}

impl Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Storage")
            .field(&self.journal)
            .field(&("pending pages", &self.pending.num_pages()))
            .finish()
    }
}

impl Storage {
    pub fn new() -> Self {
        Self {
            journal: Journal::new(),
            pending: SparsePages::new(),
        }
    }

    pub fn commit(&mut self) {
        self.journal.append(std::mem::take(&mut self.pending))
    }

    pub fn revert(&mut self) {
        self.pending.clear()
    }

    pub fn cursor(&self) -> Option<Cursor> {
        self.journal.end().ok()
    }

    pub fn sync_prepare(&self, cursor: Cursor) -> JournalPartial<SparsePages> {
        self.journal.sync_prepare(cursor, MAX_SYNC)
    }

    pub fn sync_receive(&mut self, partial: JournalPartial<SparsePages>) {
        self.journal.sync_receive(partial);
    }
}

impl sqlite_vfs::File for Storage {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        Ok(self
            .journal
            .iter()
            .flat_map(|pages| pages.max_page_idx())
            .chain(self.pending.max_page_idx())
            .max()
            .map(|n| (n + 1) * (PAGESIZE as u64))
            .unwrap_or(0))
    }

    fn truncate(&mut self, _size: u64) -> sqlite_vfs::VfsResult<()> {
        // for now we panic
        panic!("truncate not implemented")
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> sqlite_vfs::VfsResult<usize> {
        let page_idx = pos / (PAGESIZE as u64);

        // for now we panic if we attempt to write less than a full page
        assert!(buf.len() == PAGESIZE);

        let page: Page = buf.try_into().unwrap();
        self.pending.write(page_idx, page);
        Ok(buf.len())
    }

    fn read(&mut self, pos: u64, buf: &mut [u8]) -> sqlite_vfs::VfsResult<usize> {
        let page_idx = pos / (PAGESIZE as u64);
        let page_offset = (pos as usize) % PAGESIZE;

        // find the page by searching down through pending and then the journal
        let page = self.pending.read(page_idx).or_else(|| {
            self.journal
                .iter()
                .rev()
                .flat_map(|pages| pages.read(page_idx))
                .next()
        });

        if let Some(page) = page {
            // copy the page into the buffer at the offset
            let start = page_offset;
            let end = start + buf.len();
            assert!(end <= PAGESIZE);
            buf.copy_from_slice(&page[start..end]);

            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    fn sync(&mut self) -> sqlite_vfs::VfsResult<()> {
        Ok(())
    }
}
