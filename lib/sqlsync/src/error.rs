use std::io;

use thiserror::Error;

use crate::{
    reducer::ReducerError, replication::ReplicationError, timeline::TimelineError,
    JournalIdParseError,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ReplicationError(#[from] ReplicationError),

    #[error(transparent)]
    JournalIdParseError(#[from] JournalIdParseError),

    #[error(transparent)]
    TimelineError(#[from] TimelineError),

    #[error(transparent)]
    ReducerError(#[from] ReducerError),

    #[error(transparent)]
    SqliteError(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    IoError(#[from] io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
