use std::fmt::Debug;
use std::{ops::Range, slice::Iter};

use rusqlite::{types::FromSql, ToSql};

pub type LSN = u64;

/// A Cursor represents a pointer to a position in the log (LSN)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    lsn: LSN,
}

impl Cursor {
    pub fn new(lsn: LSN) -> Self {
        Self { lsn }
    }

    pub fn next(&self) -> Self {
        Self { lsn: self.lsn + 1 }
    }
}

impl ToSql for Cursor {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.lsn.to_sql()
    }
}

impl FromSql for Cursor {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(Self {
            lsn: FromSql::column_result(value)?,
        })
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

impl<T: Clone> Debug for Journal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Journal").field(&self.range).finish()
    }
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

    /// Read a partial from the journal starting at the cursor.
    /// The partial will contain at most max_len entries.
    pub fn sync_prepare<'a>(&'a self, cursor: Cursor, max_len: usize) -> JournalPartial<'a, T> {
        // start reading at the cursor
        let start_lsn = cursor.lsn;
        // TODO: these asserts should become handleable errors
        assert!(!self.data.is_empty());
        assert!(start_lsn >= self.range.start);
        assert!(start_lsn < self.range.end);
        let offset = (start_lsn - self.range.start) as usize;
        let end = std::cmp::min(offset + max_len, self.data.len());
        JournalPartial {
            start: start_lsn,
            data: &self.data[offset..end],
        }
    }

    /// Merge a partial into the journal starting at partial.start and possibly extending the journal.
    /// The partial must overlap with the journal or be immediately after the journal.
    pub fn sync_receive(&mut self, partial: JournalPartial<T>) -> Cursor {
        // TODO: these asserts should become handleable errors
        assert!(!partial.data.is_empty());
        assert!(partial.start >= self.range.start);
        assert!(partial.start <= self.range.end);
        // calculate the offset where want to inject the partial into self.data
        let offset = (partial.start - self.range.start) as usize;
        self.data = [&self.data[..offset], &partial.data].concat();
        self.range.end = partial.start + partial.data.len() as LSN;
        Cursor::new(self.range.end - 1)
    }

    /// Rollup the journal to the given cursor (inclusive), optionally compacting the entries into a new entry.
    pub fn rollup<F>(&mut self, cursor: Cursor, compactor: F)
    where
        F: FnOnce(Iter<T>) -> T,
    {
        let lsn = cursor.lsn;
        assert!(lsn >= self.range.start);
        assert!(lsn < self.range.end);
        // the + 1 is because we want to include the entry at the cursor
        let offset = (lsn - self.range.start) as usize + 1;
        let rollup = compactor(self.data[..offset].iter());
        let mut data = vec![rollup];
        data.extend_from_slice(&self.data[offset..]);
        self.data = data;
        self.range.start = lsn;
    }

    pub fn remove_up_to(&mut self, cursor: Cursor) {
        let lsn = cursor.lsn;
        assert!(lsn >= self.range.start);
        assert!(lsn < self.range.end);
        // the + 1 is because we want to include the entry at the cursor
        let offset = (lsn - self.range.start) as usize + 1;
        self.data.drain(..offset);
        self.range.start = lsn + 1;
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.data.iter()
    }

    /// Iterate over the entries in the journal in the given range inclusively (start, end)
    pub fn iter_range(&self, start: Cursor, end: Cursor) -> impl DoubleEndedIterator<Item = &T> {
        // ensure that the provided range is strictly contained by our range
        assert!(start.lsn >= self.range.start);
        assert!(start.lsn < self.range.end);
        assert!(end.lsn >= self.range.start);
        assert!(end.lsn < self.range.end);

        let start_offset = (start.lsn - self.range.start) as usize;
        let end_offset = (end.lsn - self.range.start) as usize + 1;
        self.data[start_offset..end_offset].iter()
    }
}
