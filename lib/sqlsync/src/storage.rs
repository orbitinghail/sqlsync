use std::fmt::Debug;

use super::page::{SerializedPagesReader, SparsePages, PAGESIZE};
use crate::{
    journal::{Journal, JournalIterator},
    lsn::{LsnRange, RequestedLsnRange},
    page::Page,
};

// This is the offset of the file change counter in the sqlite header which is
// stored at page 0
const FILE_CHANGE_COUNTER_OFFSET: usize = 24;

pub struct Storage<J: Journal> {
    journal: J,
    pending: SparsePages,

    file_change_counter: u32,
}

impl<J: Journal> Debug for Storage<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Storage")
            .field(&self.journal)
            .field(&("pending pages", &self.pending.num_pages()))
            .finish()
    }
}

impl<J: Journal> Storage<J> {
    pub fn new(journal: J) -> Self {
        Self {
            journal,
            pending: SparsePages::new(),
            file_change_counter: 0,
        }
    }

    pub fn commit(&mut self) -> anyhow::Result<()> {
        if self.pending.num_pages() > 0 {
            Ok(self.journal.append(std::mem::take(&mut self.pending))?)
        } else {
            Ok(())
        }
    }

    pub fn revert(&mut self) {
        self.pending.clear()
    }

    pub fn sync_prepare(&self, req: RequestedLsnRange) -> anyhow::Result<Option<J::Iter>> {
        Ok(self.journal.sync_prepare(req)?)
    }

    pub fn sync_receive(&mut self, partial: impl JournalIterator) -> anyhow::Result<LsnRange> {
        Ok(self.journal.sync_receive(partial)?)
    }
}

impl<J: Journal> sqlite_vfs::File for Storage<J> {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        Ok(self
            .journal
            .iter()
            .map_err(|_err| sqlite_vfs::SQLITE_IOERR)?
            .flat_map(|pages| SerializedPagesReader(pages).max_page_idx())
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
        // TODO: profile this - especially considering the extra clone for pending pages
        let page = match self.pending.read(page_idx) {
            Some(page) => Some(page.clone()),
            None => self
                .journal
                .iter()
                .map_err(|_| sqlite_vfs::SQLITE_IOERR)?
                .rev()
                .find_map(|pages| SerializedPagesReader(pages).read(page_idx).transpose())
                .transpose()
                .map_err(|_| sqlite_vfs::SQLITE_IOERR)?,
        };

        if let Some(page) = page {
            // copy the page into the buffer at the offset
            let start = page_offset;
            let end = start + buf.len();
            assert!(end <= PAGESIZE);
            buf.copy_from_slice(&page[start..end]);

            // disable any sqlite caching by forcing the file change
            // counter to be different every time sqlite reads the file header
            // TODO: optimize the file change counter by monitoring when sqlite
            // writes a new counter and whenever we sync from the server
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
