use std::fmt::Debug;
use std::io;

use crate::Serializable;
use crate::{
    lsn::{Lsn, LsnRange},
    JournalId,
};

use super::Scannable;

pub trait Journal: Scannable + Debug + Sized {
    type Factory: JournalFactory<Self>;

    /// this journal's id
    fn id(&self) -> JournalId;

    /// this journal's range
    fn range(&self) -> LsnRange;

    /// append a new journal entry, and then write to it
    fn append(&mut self, obj: impl Serializable) -> io::Result<()>;

    /// drop the journal's prefix
    fn drop_prefix(&mut self, up_to: Lsn) -> io::Result<()>;
}

pub trait JournalFactory<J> {
    fn open(&self, id: JournalId) -> io::Result<J>;
}
