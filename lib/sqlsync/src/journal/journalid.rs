use std::fmt::{Debug, Display, Formatter};

use bs58::Alphabet;
use rand::Rng;
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef},
    ToSql,
};
use serde::{de::Visitor, Deserialize, Serialize};

const BS58_ALPHABET: &Alphabet = bs58::Alphabet::BITCOIN;

#[derive(thiserror::Error, Debug)]
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

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum JournalId {
    Size128(Bytes128),
    Size256(Bytes256),
}

impl JournalId {
    pub fn new128(rng: &mut impl Rng) -> Self {
        let mut data = [0u8; 16];
        rng.fill(&mut data);
        Self::Size128(data)
    }

    pub fn new256(rng: &mut impl Rng) -> Self {
        let mut data = [0u8; 32];
        rng.fill(&mut data);
        Self::Size256(data)
    }

    pub fn from_base58(str: &str) -> Result<JournalId, JournalIdParseError> {
        let data = bs58::decode(str).with_alphabet(BS58_ALPHABET).into_vec()?;
        data.as_slice().try_into()
    }

    pub fn to_base58(&self) -> String {
        bs58::encode(self.bytes())
            .with_alphabet(BS58_ALPHABET)
            .into_string()
    }

    pub fn from_hex(str: &str) -> Result<JournalId, JournalIdParseError> {
        let data = hex::decode(str)?;
        data.as_slice().try_into()
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes())
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Size128(data) => data,
            Self::Size256(data) => data,
        }
    }
}

impl Debug for JournalId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_base58())
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
            16 => Self::Size128(<Bytes128>::try_from(value).unwrap()),
            32 => Self::Size256(<Bytes256>::try_from(value).unwrap()),
            len => {
                return Err(JournalIdParseError::InvalidByteLength(len));
            }
        })
    }
}

impl TryFrom<Vec<u8>> for JournalId {
    type Error = JournalIdParseError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        value.as_slice().try_into()
    }
}

impl TryFrom<&str> for JournalId {
    type Error = JournalIdParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_base58(value)
    }
}

impl Serialize for JournalId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.bytes())
    }
}

struct JournalIdVisitor;

impl JournalIdVisitor {
    fn visit_try_from<V, E>(self, v: V) -> Result<JournalId, E>
    where
        V: TryInto<JournalId, Error = JournalIdParseError>,
        E: serde::de::Error,
    {
        v.try_into().map_err(|e| match e {
            JournalIdParseError::InvalidByteLength(len) => {
                serde::de::Error::invalid_length(len, &"16 or 32 bytes")
            }
            e => serde::de::Error::custom(e),
        })
    }
}

impl<'a> Visitor<'a> for JournalIdVisitor {
    type Value = JournalId;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a journal id")
    }

    fn visit_borrowed_bytes<E>(self, v: &'a [u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_try_from(v)
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_try_from(v)
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_try_from(v)
    }

    fn visit_borrowed_str<E>(self, v: &'a str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_try_from(v)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_try_from(v)
    }
}

impl<'de> Deserialize<'de> for JournalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(JournalIdVisitor)
    }
}
