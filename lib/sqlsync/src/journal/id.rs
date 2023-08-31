use std::fmt::{Debug, Display, Formatter};

use bs58::Alphabet;
use rand::{thread_rng, Rng};
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
    ToSql,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const BS58_ALPHABET: &Alphabet = bs58::Alphabet::BITCOIN;

#[derive(Error, Debug)]
pub enum JournalIdParseError {
    #[error("failed to parse journal id; expected 16 or 32 bytes, got {0} instead")]
    InvalidByteLength(usize),

    #[error("failed to convert from base58; error: {0}")]
    Base58Error(#[from] bs58::decode::Error),

    #[error("failed to convert from hex; error: {0}")]
    HexError(#[from] hex::FromHexError),
}

type Bytes128 = [u8; 16];
type Bytes256 = [u8; 32];

#[derive(Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum JournalId {
    Raw128(Bytes128),
    Raw256(Bytes256),
}

impl JournalId {
    pub fn new128() -> Self {
        let mut data = [0u8; 16];
        thread_rng().fill(&mut data);
        Self::Raw128(data)
    }

    pub fn new256() -> Self {
        let mut data = [0u8; 32];
        thread_rng().fill(&mut data);
        Self::Raw256(data)
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Raw128(data) => data,
            Self::Raw256(data) => data,
        }
    }

    pub fn from_hex(str: &str) -> Result<JournalId, JournalIdParseError> {
        let data = hex::decode(str)?;
        data.as_slice().try_into()
    }

    pub fn from_base58(str: &str) -> Result<JournalId, JournalIdParseError> {
        let data = bs58::decode(str).with_alphabet(BS58_ALPHABET).into_vec()?;
        data.as_slice().try_into()
    }
}

impl Debug for JournalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            &bs58::encode(self.bytes())
                .with_alphabet(BS58_ALPHABET)
                .into_string(),
        )
    }
}

impl Display for JournalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self, f)
    }
}

impl ToSql for JournalId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.bytes().into())
    }
}

impl FromSql for JournalId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let bytes = <Vec<u8>>::column_result(value)?;
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| FromSqlError::InvalidType)
    }
}

impl TryFrom<&[u8]> for JournalId {
    type Error = JournalIdParseError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(match value.len() {
            16 => Self::Raw128(<Bytes128>::try_from(value).unwrap()),
            32 => Self::Raw256(<Bytes256>::try_from(value).unwrap()),
            len => {
                return Err(JournalIdParseError::InvalidByteLength(len));
            }
        })
    }
}
