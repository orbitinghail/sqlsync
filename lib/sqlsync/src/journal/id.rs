use std::{
    fmt::{Debug, Display, Formatter},
    ops::Deref,
};

use rusqlite::{
    types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef},
    ToSql,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
#[error(transparent)]
pub struct JournalIdParseError(#[from] uuid::Error);

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct JournalId(Uuid);

impl JournalId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Debug for JournalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl Display for JournalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl Deref for JournalId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ToSql for JournalId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.0.as_bytes()[..].into())
    }
}

impl FromSql for JournalId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let bytes = <[u8; 16]>::column_result(value)?;
        Ok(Self(Uuid::from_bytes(bytes)))
    }
}

impl TryFrom<String> for JournalId {
    type Error = JournalIdParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(Self(Uuid::try_parse(&value)?))
    }
}
