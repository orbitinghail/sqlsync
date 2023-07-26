use std::io;

use thiserror::Error;

use crate::{lsn::SatisfyError, LsnRange};

use super::JournalId;

pub type JournalResult<T> = std::result::Result<T, JournalError>;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("refusing to sync from empty partial")]
    EmptyPartial,

    #[error("failed to open journal, error: {0}")]
    FailedToOpenJournal(#[source] anyhow::Error),

    #[error("failed to prepare journal partial from request: {0}")]
    FailedToPrepareRequest(#[source] SatisfyError),

    #[error("io error: {0}")]
    IoError(#[from] io::Error),

    #[error("journal range {journal_debug} does not intersect or preceed partial range {partial_range:?}")]
    RangesMustBeContiguous {
        journal_debug: String,
        partial_range: LsnRange,
    },

    #[error("failed to serialize object")]
    SerializationError(#[source] anyhow::Error),

    #[error("refusing to sync from journal id {partial_id} into journal id {self_id}")]
    WrongJournal {
        partial_id: JournalId,
        self_id: JournalId,
    },
}
