use std::io;
use thiserror::Error;

use crate::{
    lsn::SatisfyError,
    positioned_io::{PositionedCursor, PositionedReader},
    JournalId, LsnRange, RequestedLsnRange,
};

use super::cursor::Cursor;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error(transparent)]
    IoError(#[from] io::Error),

    #[error("failed to prepare journal partial from request: {0}")]
    FailedToPrepareRequest(#[source] SatisfyError),

    #[error("journal range {journal_debug} does not intersect or preceed partial range {partial_range:?}")]
    RangesMustBeContiguous {
        journal_debug: String,
        partial_range: LsnRange,
    },

    #[error("refusing to sync from journal {from_id} into journal {self_id}")]
    WrongJournal {
        from_id: JournalId,
        self_id: JournalId,
    },

    #[error("transparent")]
    Unknown(#[from] anyhow::Error),
}

pub type SyncResult<T> = Result<T, SyncError>;

pub trait Syncable {
    type Cursor<'a>: Cursor
    where
        Self: 'a;

    /// this object's source journal id
    fn source_id(&self) -> JournalId;

    /// prepare a journal partial for the given request from our source journal
    fn sync_prepare<'a>(
        &'a mut self,
        req: RequestedLsnRange,
    ) -> SyncResult<Option<JournalPartial<Self::Cursor<'a>>>>;

    /// build a request for the specified journal id
    fn sync_request(&mut self, id: JournalId) -> SyncResult<RequestedLsnRange>;

    /// receive a journal partial from a remote journal
    fn sync_receive<C>(&mut self, partial: JournalPartial<C>) -> SyncResult<LsnRange>
    where
        C: Cursor + io::Read;
}

pub struct JournalPartial<C: Cursor> {
    id: JournalId,
    range: LsnRange,
    cursor: C,
}

impl<C: Cursor> JournalPartial<C> {
    pub fn new(id: JournalId, range: LsnRange, cursor: C) -> Self {
        Self { id, range, cursor }
    }

    pub fn id(&self) -> JournalId {
        self.id
    }

    pub fn range(&self) -> LsnRange {
        self.range
    }

    pub fn len(&self) -> usize {
        self.range.len()
    }

    pub fn into_cursor(self) -> C {
        self.cursor
    }
}

// conversion from JournalPartial<Cursor+PositionedReader> into JournalPartial<Cursor+io::Read>
// useful for testing and local end-to-end demos
impl<T: Cursor + PositionedReader> JournalPartial<T> {
    pub fn into_read_partial(self) -> JournalPartial<PositionedCursor<T>> {
        JournalPartial::new(self.id, self.range, PositionedCursor::new(self.cursor))
    }
}
