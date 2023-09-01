use thiserror::Error;

use crate::{
    reducer::ReducerError, replication::ReplicationError, timeline::TimelineError, JournalError,
    JournalIdParseError, ScanError,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ReplicationError(#[from] ReplicationError),

    #[error(transparent)]
    ScanError(#[from] ScanError),

    #[error(transparent)]
    JournalError(#[from] JournalError),

    #[error(transparent)]
    JournalIdParseError(#[from] JournalIdParseError),

    #[error(transparent)]
    TimelineError(#[from] TimelineError),

    #[error(transparent)]
    ReducerError(#[from] ReducerError),

    #[error(transparent)]
    SqliteError(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
