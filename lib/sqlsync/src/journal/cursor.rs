use std::{io, result::Result};
use thiserror::Error;

use crate::{lsn::LsnRange, positioned_io::PositionedReader};

#[derive(Error, Debug)]
pub enum ScanError {
    #[error(transparent)]
    Io(#[from] io::Error),
}

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
    fn advance(&mut self) -> Result<bool, ScanError>;

    /// return the number of advances remaining in the scan
    fn remaining(&self) -> usize;

    /// reverse this cursor
    fn into_rev(self) -> Self;
}

pub trait Scannable {
    type Cursor<'a>: Cursor + PositionedReader
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Self::Cursor<'a>;
    fn scan_range<'a>(&'a self, range: LsnRange) -> Self::Cursor<'a>;
}
