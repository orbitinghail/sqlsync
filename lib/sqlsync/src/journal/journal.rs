use std::fmt::Debug;

use crate::lsn::{Lsn, LsnRange, RequestedLsnRange};
use crate::positioned_io::PositionedReader;
use crate::Serializable;

use super::error::JournalResult;

pub type JournalId = i64;

pub trait JournalIterator: DoubleEndedIterator<Item = Self::Entry> {
    type Entry: PositionedReader;

    fn id(&self) -> JournalId;
    fn range(&self) -> Option<LsnRange>;

    fn is_empty(&self) -> bool {
        self.range().is_none()
    }
}

pub trait Journal: Debug + Sized {
    type Iter: JournalIterator;

    fn open(id: JournalId) -> JournalResult<Self>;

    // TODO: eventually this needs to be a UUID of some kind
    fn id(&self) -> JournalId;

    /// append a new journal entry, and then write to it
    fn append(&mut self, obj: impl Serializable) -> JournalResult<()>;

    /// iterate over journal entries
    fn iter(&self) -> JournalResult<Self::Iter>;
    fn iter_range(&self, range: LsnRange) -> JournalResult<Self::Iter>;

    /// sync
    fn sync_prepare(&self, req: RequestedLsnRange) -> JournalResult<Option<Self::Iter>>;

    fn sync_receive(&mut self, partial: impl JournalIterator) -> JournalResult<LsnRange>;

    /// drop the journal's prefix
    fn drop_prefix(&mut self, up_to: Lsn) -> JournalResult<()>;
}
