use std::{
    io::{self, Result},
    iter::Rev,
};

use crate::{
    lsn::{LsnIter, LsnRange},
    positioned_io::PositionedReader,
    Lsn,
};

pub trait Scannable: Sized {
    type Reader<'a>: PositionedReader
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Cursor<'a, Self, LsnIter>;
    fn scan_range<'a>(&'a self, range: LsnRange) -> Cursor<'a, Self, LsnIter>;

    fn get<'a>(&'a self, lsn: Lsn) -> Result<Option<Self::Reader<'a>>>;
}

pub struct Cursor<'a, S: Scannable, I> {
    inner: &'a S,
    lsn_iter: I,
    state: Option<(Lsn, S::Reader<'a>)>,
}

impl<'a, S: Scannable, I: DoubleEndedIterator<Item = Lsn>> Cursor<'a, S, I> {
    pub fn new(inner: &'a S, lsn_iter: I) -> Self {
        Self { inner, lsn_iter, state: None }
    }

    /// advance the cursor
    /// Note: you must call advance() once to start reading the first entry
    ///
    /// example:
    ///     let mut cursor = journal.scan();
    ///     while cursor.advance()? {
    ///         ... cursor.read_at(...)
    ///     }
    pub fn advance(&mut self) -> Result<bool> {
        if let Some(lsn) = self.lsn_iter.next() {
            let reader = self.inner.get(lsn)?.expect("cursor out of sync");
            self.state = Some((lsn, reader));
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// return the lsn the cursor is currently pointing at
    pub fn lsn(&mut self) -> Option<Lsn> {
        self.state.as_ref().map(|(lsn, _)| *lsn)
    }

    /// reverse this cursor
    pub fn into_rev(self) -> Cursor<'a, S, Rev<I>> {
        Cursor { inner: self.inner, lsn_iter: self.lsn_iter.rev(), state: None }
    }
}

impl<'a, S: Scannable, I> PositionedReader for Cursor<'a, S, I> {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        match self.state {
            None => Ok(0),
            Some((_, ref reader)) => reader.read_at(pos, buf),
        }
    }

    fn size(&self) -> io::Result<usize> {
        match self.state {
            None => Ok(0),
            Some((_, ref reader)) => reader.size(),
        }
    }
}
