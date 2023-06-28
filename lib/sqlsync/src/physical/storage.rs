use super::{page::SparsePages, PAGESIZE};
use crate::{
    journal::{Batch, Cursor, Journal},
    physical::page::Page,
};
use anyhow::Result;

const MAX_BATCH_SIZE: usize = 10;

pub struct Storage {
    journal: Journal<SparsePages>,
    pending: SparsePages,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            journal: Journal::new(),
            pending: SparsePages::new(),
        }
    }

    pub fn cursor(&self) -> Result<Cursor> {
        self.journal.end()
    }

    pub fn commit(&mut self) {
        self.journal.append(std::mem::take(&mut self.pending))
    }

    pub fn revert(&mut self) {
        self.pending.clear()
    }

    pub fn receive_batch(&mut self, batch: Batch<SparsePages>) {
        self.journal.write(batch);
    }

    pub fn prepare_batch(&self, cursor: Cursor) -> Batch<SparsePages> {
        self.journal.read(cursor, MAX_BATCH_SIZE)
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

    fn truncate(&mut self, size: u64) -> sqlite_vfs::VfsResult<()> {
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
