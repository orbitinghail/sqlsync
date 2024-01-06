use std::{collections::HashSet, fmt::Debug, io};

use serde::{Deserialize, Serialize};
use sqlite_vfs::SQLITE_IOERR;

use super::page::{SerializedPagesReader, SparsePages, PAGESIZE};
use crate::{
    journal::Journal,
    lsn::LsnRange,
    page::{Page, PageIdx},
    replication::{ReplicationDestination, ReplicationSource},
    Lsn,
};

// Useful SQLite header offsets
// The SQLite header is the first 100 bytes in page 0
// The following offsets are relative to the start of the header

// We use the file change counter to control SQLite caching
const FILE_CHANGE_COUNTER_OFFSET: usize = 24;

// The schema cookie is used to determine if the schema has changed
const SCHEMA_COOKIE_OFFSET: usize = 40;

/// StorageChange specifies the type of change that occurred in storage
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum StorageChange {
    /// Either the schema has changed, or so much of the storage has changed that it's not worth tracking
    /// All caches or query subscriptions should be invalidated
    Full,

    /// one or more table btrees have changed
    /// the root page indexes for each table are provided
    Tables { root_pages_sorted: Vec<PageIdx> },
}

pub struct Storage<J> {
    journal: J,
    visible_lsn_range: LsnRange,
    pending: SparsePages,

    file_change_counter: u32,

    // the following three fields are reset whenever Storage::changes() is called
    last_schema_cookie: u32,
    changed_root_pages: HashSet<PageIdx>,
    changed_pages: HashSet<PageIdx>,
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
        let visible_lsn_range = journal.range();
        Self {
            journal,
            visible_lsn_range,
            pending: SparsePages::new(),
            file_change_counter: 0,
            last_schema_cookie: 0,
            changed_root_pages: HashSet::new(),
            changed_pages: HashSet::new(),
        }
    }

    pub fn last_committed_lsn(&self) -> Option<Lsn> {
        self.journal.range().last()
    }

    pub fn has_committed_pages(&self) -> bool {
        self.journal.range().is_non_empty()
    }

    pub fn has_invisible_pages(&self) -> bool {
        self.visible_lsn_range.last() < self.journal.range().last()
    }

    pub fn commit(&mut self) -> io::Result<()> {
        if self.pending.num_pages() > 0 {
            self.journal.append(std::mem::take(&mut self.pending))?;

            // calculate the LsnRange between the current visible range and the committed range
            let new_lsns =
                self.journal.range().difference(&self.visible_lsn_range);
            // clear the changed pages list (update_changed_root_pages will scan the new lsns)
            self.changed_pages.clear();

            // update the visible range
            self.visible_lsn_range = self.journal.range();
            // update the file change counter
            self.file_change_counter = self.file_change_counter.wrapping_add(1);

            // update changed root pages in the newly visible range
            self.update_changed_root_pages(new_lsns)?;
        }
        Ok(())
    }

    pub fn reset(&mut self) -> io::Result<()> {
        // mark every page in pending as changed to ensure that we re-run queries that depended on the results of something in pending
        self.changed_pages = self.pending.page_idxs().copied().collect();

        // clear pending to revert uncommitted changes
        self.pending.clear();

        // calculate the LsnRange between the current visible range and the committed range
        let new_lsns = self.journal.range().difference(&self.visible_lsn_range);

        // update the visible range to reveal committed changes
        self.visible_lsn_range = self.journal.range();
        // update the file change counter
        self.file_change_counter = self.file_change_counter.wrapping_add(1);

        // update changed root pages in the newly visible range
        self.update_changed_root_pages(new_lsns)?;

        Ok(())
    }

    /// update_changed_root_pages does two things
    /// 1. it scans the journal, updating changed_root_pages for each frame
    /// 2. it updates changed_root_pages for every page in self.changed_pages
    fn update_changed_root_pages(&mut self, range: LsnRange) -> io::Result<()> {
        // scan the journal, updating changed_root_pages for each frame
        let mut cursor = self.journal.scan_range(range);
        while cursor.advance()? {
            let lsn = cursor.lsn().unwrap();
            let pages = SerializedPagesReader(&cursor);
            for page_idx in pages.page_idxs()?.iter() {
                // we need to resolve each page_idx to it's root page by only
                // looking at ptrmap pages that existed as of this lsn
                if let Some(root_page_idx) = self.resolve_root_page(
                    LsnRange::new(0, lsn),
                    false,
                    *page_idx,
                )? {
                    self.changed_root_pages.insert(root_page_idx);
                }
            }
        }

        // finally, if we have any changed pages, update changed_root_pages for each page
        for page_idx in self.changed_pages.iter() {
            // we need to resolve each page_idx to it's root page by only
            // looking at ptrmap pages that existed as of the last visible lsn
            if let Some(root_page_idx) =
                self.resolve_root_page(self.visible_lsn_range, true, *page_idx)?
            {
                self.changed_root_pages.insert(root_page_idx);
            }
        }

        // clear changed_pages
        self.changed_pages.clear();

        Ok(())
    }

    /// resolve_root_page returns the root page index for the given page at the
    /// given lsn range (potentially including pending pages)
    /// if the page does not map to a b-tree root page, then None is returned
    fn resolve_root_page(
        &self,
        range: LsnRange,
        include_pending: bool,
        page_idx: PageIdx,
    ) -> io::Result<Option<PageIdx>> {
        const PENDING_BYTE_PAGE_IDX: u64 = (0x40000000 / (PAGESIZE as u64)) + 1;

        // XXX: SQLSync does not currently support SQLite extensions, so we
        // calculate usable page size == PAGESIZE
        // If we ever support SQLite extensions this will need to be updated to
        // take into account the reserved region for extensions at the end of
        // each page
        const USABLE_PAGE_SIZE: u64 = PAGESIZE as u64;

        const PTRMAP_ENTRY_SIZE: u64 = 5;

        // when calculating PAGES_PER_PTRMAP we add 1 to make the math nicer by
        // effectively taking into account the ptrmap page itself
        // math mostly copied from:
        //  https://github.com/sqlite/sqlite/blob/1eca330a08e18fd0930491302802141f5ce6298e/src/btree.c#L989C1-L1001C2
        const PAGES_PER_PTRMAP: u64 =
            (USABLE_PAGE_SIZE / PTRMAP_ENTRY_SIZE) + 1;

        if page_idx == 1 {
            // page 1 is the schema root page
            return Ok(Some(1));
        }

        let mut page_idx = page_idx as u64;
        let mut ptrmap_entry = [0u8; PTRMAP_ENTRY_SIZE as usize];
        loop {
            // which ptrmap are we referring to
            let ptrmap_n = (page_idx - 2) / PAGES_PER_PTRMAP;
            // what is the page index of the ptrmap
            let mut ptrmap_page_idx = (ptrmap_n * PAGES_PER_PTRMAP) + 2;

            if ptrmap_page_idx == PENDING_BYTE_PAGE_IDX {
                // for certain usable page sizes, it's possible for a ptrmap
                // page to share the same location as the pending byte lock page
                // in this case, sqlite simply moves the ptrmap to the next page
                // all other ptrmap locations are unchanged
                ptrmap_page_idx += 1;
            }

            if ptrmap_page_idx == page_idx {
                // looking for a ptrmap, no root page
                return Ok(None);
            }

            // calculate the offset of the page_idx within the ptrmap page
            let page_idx_offset =
                (page_idx - ptrmap_page_idx - 1) * PTRMAP_ENTRY_SIZE;
            // convert the relative offset to an absolute offset within the file
            let page_idx_pos =
                ((ptrmap_page_idx - 1) * (PAGESIZE as u64)) + page_idx_offset;

            // read the ptrmap_entry for this page
            self.read_at_range(
                range,
                include_pending,
                page_idx_pos,
                &mut ptrmap_entry,
            )?;
            match ptrmap_entry[0] {
                0 => {
                    // page is missing, this can happen while we are rebasing
                    // right after we create a local table or index (for example)
                    return Ok(None);
                }
                1 => {
                    // page is a b-tree root page
                    // return the page_idx
                    return Ok(Some(page_idx as PageIdx));
                }
                2 => {
                    // page is a freelist page
                    return Ok(None);
                }
                _ => {
                    // ptrmap entry points at the next page in the chain
                    page_idx = u32::from_be_bytes([
                        ptrmap_entry[1],
                        ptrmap_entry[2],
                        ptrmap_entry[3],
                        ptrmap_entry[4],
                    ]) as u64;
                }
            }
        }
    }

    fn schema_cookie(&self) -> io::Result<u32> {
        let mut buf = [0; 4];
        self.read_at_range(
            self.visible_lsn_range,
            true,
            SCHEMA_COOKIE_OFFSET as u64,
            &mut buf,
        )?;
        Ok(u32::from_be_bytes(buf))
    }

    pub fn has_changes(&self) -> bool {
        // it's not possible for the schema to change without also modifying pages
        // so we don't have to check the schema cookie here
        return self.changed_pages.len() > 0
            || self.changed_root_pages.len() > 0;
    }

    pub fn changes(&mut self) -> io::Result<StorageChange> {
        // check to see if the schema has changed
        let schema_cookie = self.schema_cookie()?;
        if schema_cookie != self.last_schema_cookie {
            log::info!(
                "schema changed: {} -> {}",
                self.last_schema_cookie,
                schema_cookie
            );
            self.last_schema_cookie = schema_cookie;
            self.changed_root_pages.clear();
            self.changed_pages.clear();
            return Ok(StorageChange::Full);
        }

        // if the schema hasn't changed, then we need to trace which btrees have changed

        // accumulate any outstanding pages into changed_root_pages
        self.update_changed_root_pages(LsnRange::empty())?;

        // gather changed root pages into sorted vec
        let mut root_pages_sorted: Vec<_> =
            self.changed_root_pages.iter().copied().collect();
        root_pages_sorted.sort();

        // reset variables
        self.last_schema_cookie = schema_cookie;
        self.changed_root_pages.clear();
        self.changed_pages.clear();

        Ok(StorageChange::Tables { root_pages_sorted })
    }

    fn read_at_range(
        &self,
        range: LsnRange,
        include_pending: bool,
        pos: u64,
        buf: &mut [u8],
    ) -> io::Result<usize> {
        let page_idx = ((pos / (PAGESIZE as u64)) + 1) as PageIdx;
        let page_offset = (pos as usize) % PAGESIZE;

        // find the page by searching down through pending and then the journal
        let mut n = if include_pending {
            self.pending.read(page_idx, page_offset, buf)
        } else {
            0
        };

        let mut cursor = self.journal.scan_range(range).into_rev();
        while n == 0 && cursor.advance()? {
            let pages = SerializedPagesReader(&cursor);
            n = pages.read(page_idx, page_offset, buf)?;
        }

        if n != 0 {
            assert!(n == buf.len(), "read should always fill the buffer");

            // if SQLite is potentially reading the file change counter
            // replace what is in the file with the current value
            if page_idx == 1
                && page_offset <= FILE_CHANGE_COUNTER_OFFSET
                && page_offset + buf.len() >= FILE_CHANGE_COUNTER_OFFSET + 4
            {
                // if pos = 0, then this should be FILE_CHANGE_COUNTER_OFFSET
                // if pos = FILE_CHANGE_COUNTER_OFFSET, this this should be 0
                let file_change_buf_offset =
                    FILE_CHANGE_COUNTER_OFFSET - page_offset;

                buf[file_change_buf_offset..(file_change_buf_offset + 4)]
                    .copy_from_slice(&self.file_change_counter.to_be_bytes());
            }

            Ok(buf.len())
        } else {
            Ok(0)
        }
    }
}

impl<J: ReplicationSource> ReplicationSource for Storage<J> {
    type Reader<'a> = <J as ReplicationSource>::Reader<'a>
    where
        Self: 'a;

    fn source_id(&self) -> crate::JournalId {
        self.journal.source_id()
    }

    fn source_range(&self) -> crate::LsnRange {
        self.journal.source_range()
    }

    fn read_lsn<'a>(
        &'a self,
        lsn: crate::Lsn,
    ) -> io::Result<Option<Self::Reader<'a>>> {
        self.journal.read_lsn(lsn)
    }
}

impl<J: ReplicationDestination> ReplicationDestination for Storage<J> {
    fn range(
        &mut self,
        id: crate::JournalId,
    ) -> Result<LsnRange, crate::replication::ReplicationError> {
        self.journal.range(id)
    }

    fn write_lsn<R>(
        &mut self,
        id: crate::JournalId,
        lsn: crate::Lsn,
        reader: &mut R,
    ) -> Result<(), crate::replication::ReplicationError>
    where
        R: io::Read,
    {
        self.journal.write_lsn(id, lsn, reader)
    }
}

impl<J: Journal> sqlite_vfs::File for Storage<J> {
    fn file_size(&self) -> sqlite_vfs::VfsResult<u64> {
        let mut max_page_idx = self.pending.max_page_idx();

        // if we have visible lsns in storage, then we need to scan them
        // to find the max page idx
        let mut cursor = self.journal.scan_range(self.visible_lsn_range);
        while cursor.advance().map_err(|_| SQLITE_IOERR)? {
            let pages = SerializedPagesReader(&cursor);
            max_page_idx = max_page_idx
                .max(Some(pages.max_page_idx().map_err(|_| SQLITE_IOERR)?));
        }

        Ok(max_page_idx
            .map(|n| (n as u64) * (PAGESIZE as u64))
            .unwrap_or(0))
    }

    fn truncate(&mut self, _size: u64) -> sqlite_vfs::VfsResult<()> {
        // for now we panic
        panic!("truncate not implemented")
    }

    fn write(&mut self, pos: u64, buf: &[u8]) -> sqlite_vfs::VfsResult<usize> {
        let page_idx = ((pos / (PAGESIZE as u64)) + 1) as PageIdx;
        log::debug!("writing page {}", page_idx);

        // for now we panic if we attempt to write less than a full page
        assert!(buf.len() == PAGESIZE);

        let page: Page = buf.try_into().unwrap();
        self.pending.write(page_idx, page);

        // update the file change counter
        self.file_change_counter = self.file_change_counter.wrapping_add(1);

        // mark the page as changed
        self.changed_pages.insert(page_idx);

        Ok(buf.len())
    }

    fn read(
        &mut self,
        pos: u64,
        buf: &mut [u8],
    ) -> sqlite_vfs::VfsResult<usize> {
        self.read_at_range(self.visible_lsn_range, true, pos, buf)
            .map_err(|_| SQLITE_IOERR)
    }

    fn sync(&mut self) -> sqlite_vfs::VfsResult<()> {
        Ok(())
    }
}
