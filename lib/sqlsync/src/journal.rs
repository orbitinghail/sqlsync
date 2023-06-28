use std::{ops::Range, slice::Iter};

pub type LSN = u64;

/// A Cursor represents a pointer to a position in the log (LSN)
pub struct Cursor {
    lsn: LSN,
}

impl Cursor {
    pub fn new(lsn: LSN) -> Self {
        Self { lsn }
    }
}

pub struct JournalPartial<'a, T> {
    start: LSN,
    data: &'a [T],
}

impl<T> JournalPartial<'_, T> {
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub struct Journal<T>
where
    T: Clone,
{
    /// The range of LSNs covered by this journal.
    /// The journal is guaranteed to contain all LSNs in the range [start, end).
    range: Range<LSN>,
    data: Vec<T>,
}

impl<T> Journal<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        Self {
            range: 0..0,
            data: Vec::new(),
        }
    }

    /// Return a cursor pointing at the last entry in the journal.
    pub fn end(&self) -> anyhow::Result<Cursor> {
        if self.data.is_empty() {
            Err(anyhow::anyhow!("Journal is empty"))
        } else {
            Ok(Cursor::new(self.range.end - 1))
        }
    }

    /// Append a single entry to the journal.
    pub fn append(&mut self, entry: T) {
        self.data.push(entry);
        self.range.end += 1;
    }

    /// Read a partial from the journal starting *after* cursor.
    /// The partial will contain at most max_len entries.
    pub fn sync_prepare<'a>(&'a self, cursor: Cursor, max_len: usize) -> JournalPartial<'a, T> {
        // start reading after the cursor
        let start_lsn = cursor.lsn + 1;
        // TODO: these asserts should become handleable errors
        assert!(!self.data.is_empty());
        assert!(start_lsn >= self.range.start);
        assert!(start_lsn < self.range.end);
        let offset = start_lsn - self.range.start;
        let end = std::cmp::min(offset + max_len as LSN, self.range.end);
        JournalPartial {
            start: start_lsn,
            data: &self.data[offset as usize..end as usize],
        }
    }

    /// Merge a partial into the journal starting at partial.start and possibly extending the journal.
    /// The partial must overlap with the journal or be immediately after the journal.
    /// Note: this method does not replace existing entries in the journal, it only extends the journal if needed.
    pub fn sync_receive(&mut self, partial: JournalPartial<T>) -> Cursor {
        // TODO: these asserts should become handleable errors
        assert!(!partial.data.is_empty());
        assert!(partial.start >= self.range.start);
        assert!(partial.start <= self.range.end);
        let offset = self.range.end - partial.start;
        self.data
            .extend_from_slice(&partial.data[offset as usize..]);
        self.range.end = partial.start + partial.data.len() as LSN;
        Cursor::new(self.range.end - 1)
    }

    /// Rollup the journal to the given cursor, optionally compacting the entries into a new entry.
    pub fn rollup<F>(&mut self, cursor: Cursor, cb: Option<F>)
    where
        F: FnOnce(Iter<T>) -> T,
    {
        let lsn = cursor.lsn;
        assert!(lsn >= self.range.start);
        assert!(lsn <= self.range.end);
        let offset = (lsn - self.range.start) as usize;

        if let Some(compactor) = cb {
            let rollup = compactor(self.data[offset..].iter());
            let mut data = vec![rollup];
            data.extend_from_slice(&self.data[offset..]);
            self.data = data;
        } else {
            self.data.drain(0..offset);
        }
        self.range.start = lsn;
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.data.iter()
    }
}
