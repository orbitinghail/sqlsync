use std::fmt::Debug;
use std::io;
use std::result::Result;

use thiserror::Error;

use crate::Serializable;
use crate::{lsn::Lsn, JournalId};

use super::{Scannable, Syncable};

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("failed to open journal, error: {0}")]
    FailedToOpenJournal(#[source] anyhow::Error),

    #[error("io error: {0}")]
    IoError(#[from] io::Error),

    #[error("failed to serialize object")]
    SerializationError(#[source] io::Error),
}

pub type JournalResult<T> = Result<T, JournalError>;

pub trait Journal: Syncable + Scannable + Debug + Sized {
    fn open(id: JournalId) -> JournalResult<Self>;

    /// this journal's id
    fn id(&self) -> JournalId;

    /// append a new journal entry, and then write to it
    fn append(&mut self, obj: impl Serializable) -> JournalResult<()>;

    /// drop the journal's prefix
    fn drop_prefix(&mut self, up_to: Lsn) -> JournalResult<()>;
}
