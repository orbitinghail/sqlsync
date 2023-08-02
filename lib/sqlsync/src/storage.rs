use std::{fmt::Debug, io};

use sqlite_vfs::SQLITE_IOERR;

use super::page::{SerializedPagesReader, SparsePages, PAGESIZE};
use crate::{
    journal::{Cursor, Journal, JournalPartial, SyncResult, Syncable},
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
}

impl<J: Journal> Syncable for Storage<J> {
    type Cursor<'a> = <J as Syncable>::Cursor<'a> where Self: 'a;

    fn source_id(&self) -> crate::JournalId {
        self.journal.id()
    }

    fn sync_prepare<'a>(
        &'a mut self,
        req: RequestedLsnRange,
    ) -> SyncResult<Option<JournalPartial<Self::Cursor<'a>>>> {
        self.journal.sync_prepare(req)
    }

    fn sync_request(&mut self, id: crate::JournalId) -> SyncResult<RequestedLsnRange> {
        self.journal.sync_request(id)
    }

    fn sync_receive<C>(&mut self, partial: JournalPartial<C>) -> SyncResult<LsnRange>
    where
        C: Cursor + io::Read,
    {
        self.journal.sync_receive(partial)
    }
}

impl<J: Journal> sqlite_vfs::File for Storage<J> {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        let mut max_page_idx = self.pending.max_page_idx();
        let mut cursor = self.journal.scan();
        while cursor.advance().map_err(|_| SQLITE_IOERR)? {
            let pages = SerializedPagesReader(&cursor);
            max_page_idx = max_page_idx.max(Some(pages.max_page_idx().map_err(|_| SQLITE_IOERR)?));
        }

        Ok(max_page_idx
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
        let mut n = self.pending.read(page_idx, page_offset, buf);
        let mut cursor = self.journal.scan_rev();
        while n == 0 && cursor.advance().map_err(|_| SQLITE_IOERR)? {
            let pages = SerializedPagesReader(&cursor);
            n = pages
                .read(page_idx, page_offset, buf)
                .map_err(|_| SQLITE_IOERR)?;
        }

        if n != 0 {
            assert!(n == buf.len(), "read should always fill the buffer");

            // disable any sqlite caching by forcing the file change
            // counter to be different every time sqlite reads the file header
            // TODO: optimize the file change counter by monitoring when sqlite
            // writes a new counter and whenever we sync from the server
            if page_idx == 0
                && page_offset <= FILE_CHANGE_COUNTER_OFFSET
                && page_offset + buf.len() >= FILE_CHANGE_COUNTER_OFFSET + 4
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
