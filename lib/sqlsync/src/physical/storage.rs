use std::{
    collections::hash_map::DefaultHasher,
    fmt::Debug,
    hash::{Hash, Hasher},
    time::Instant,
};

use super::{page::SparsePages, PAGESIZE};
use crate::{
    journal::{Journal, JournalPartial},
    lsn::{LsnRange, RequestedLsnRange},
    physical::page::Page,
};

// TODO: eventually we should decide on MAX_SYNC based on the number of pages
// per journal entry
// idea: push this down to Journal via some kind of "include" callback to decide
// which entries to include in order (once it returns false, return the partial)
const MAX_SYNC: usize = 1;

// This is the offset of the file change counter in the sqlite header which is
// stored at page 0
const FILE_CHANGE_COUNTER_OFFSET: usize = 24;

pub struct Storage {
    journal: Journal<SparsePages>,
    pending: SparsePages,

    file_change_counter: u32,
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
            file_change_counter: 0,
        }
    }

    pub fn commit(&mut self) {
        self.journal.append(std::mem::take(&mut self.pending))
    }

    pub fn revert(&mut self) {
        self.pending.clear()
    }

    pub fn sync_request(&self) -> RequestedLsnRange {
        self.journal.sync_request(MAX_SYNC)
    }

    pub fn sync_prepare(&self, req: RequestedLsnRange) -> Option<JournalPartial<SparsePages>> {
        self.journal.sync_prepare(req)
    }

    pub fn sync_receive(
        &mut self,
        partial: JournalPartial<SparsePages>,
    ) -> anyhow::Result<LsnRange> {
        self.journal.sync_receive(partial)
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
        log::debug!("writing page {}", page_idx);

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
                .enumerate()
                .flat_map(|(i, pages)| {
                    log::debug!("read: searching journal entry {} for page {}", i, page_idx);
                    pages.read(page_idx)
                })
                .next()
        });

        if let Some(page) = page {
            // copy the page into the buffer at the offset
            let start = page_offset;
            let end = start + buf.len();
            assert!(end <= PAGESIZE);
            buf.copy_from_slice(&page[start..end]);

            // disable any sqlite caching by forcing the file change
            // counter to be different every time sqlite reads the file header
            if page_idx == 0
                && start <= FILE_CHANGE_COUNTER_OFFSET
                && end >= FILE_CHANGE_COUNTER_OFFSET + 4
            {
                // if pos = 0, then this should be FILE_CHANGE_COUNTER_OFFSET
                // if pos = FILE_CHANGE_COUNTER_OFFSET, this this should be 0
                let file_change_buf_offset = FILE_CHANGE_COUNTER_OFFSET - page_offset;

                // we only care that *each time* sqlite tries to read the first
                // page, it sees a different file change counter. So we can just
                // bit flip self.file_change_counter and write it into the header
                self.file_change_counter ^= 1;
                buf[file_change_buf_offset..(file_change_buf_offset + 4)]
                    .copy_from_slice(&self.file_change_counter.to_be_bytes());
            }

            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    fn sync(&mut self) -> sqlite_vfs::VfsResult<()> {
        Ok(())
    }
}
