use std::io;

use crate::{positioned_io::PositionedReader, LsnRange};

pub trait Cursor {
    /// advance the cursor
    /// Note: the cursor begins at the start of the scan, so you must call
    /// advance() once to start reading the first entry
    ///
    /// example:
    ///     let mut cursor = journal.scan();
    ///     while cursor.advance()? {
    ///         ... cursor.read() ...
    ///     }
    fn advance(&mut self) -> io::Result<bool>;

    /// return the number of advances remaining in the scan
    fn remaining(&self) -> usize;
}

pub trait Scannable {
    type Cursor<'a>: Cursor + PositionedReader
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Self::Cursor<'a>;
    fn scan_rev<'a>(&'a self) -> Self::Cursor<'a>;
    fn scan_range<'a>(&'a self, range: LsnRange) -> Self::Cursor<'a>;
}
