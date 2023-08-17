use std::{
    collections::BTreeMap,
    convert::From,
    error::Error,
    fmt::{self, Display, Formatter},
    panic,
    str::FromStr,
};

use log::Level;
use serde::{Deserialize, Serialize};

pub type RequestId = u32;

pub type Requests = Option<BTreeMap<RequestId, Request>>;
pub type Responses = Option<BTreeMap<RequestId, u32>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SqliteValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Row(Vec<SqliteValue>);

impl From<Vec<SqliteValue>> for Row {
    fn from(v: Vec<SqliteValue>) -> Self {
        Self(v)
    }
}

impl FromIterator<SqliteValue> for Row {
    fn from_iter<T: IntoIterator<Item = SqliteValue>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Query {
        sql: String,
        params: Vec<SqliteValue>,
    },
    Exec {
        sql: String,
        params: Vec<SqliteValue>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExecResponse {
    pub changes: usize,
}

#[derive(Serialize, Deserialize)]
pub struct LogRecord {
    level: String,
    message: String,
    file: Option<String>,
    line: Option<u32>,
}

impl From<&panic::PanicInfo<'_>> for LogRecord {
    fn from(info: &panic::PanicInfo) -> Self {
        let loc = info.location();
        LogRecord {
            level: log::Level::Error.to_string(),
            message: info.to_string(),
            file: loc.map(|l| l.file().to_string()),
            line: loc.map(|l| l.line()),
        }
    }
}

impl From<&log::Record<'_>> for LogRecord {
    fn from(record: &log::Record) -> Self {
        Self {
            level: record.level().to_string(),
            message: record.args().to_string(),
            file: record.file().map(|s| s.to_string()),
            line: record.line(),
        }
    }
}

impl LogRecord {
    pub fn log(&self) {
        log::logger().log(
            &log::Record::builder()
                .level(Level::from_str(&self.level).unwrap_or(Level::Error))
                .file(self.file.as_ref().map(|s| s.as_str()))
                .line(self.line)
                .module_path(Some("wasm guest"))
                .args(format_args!("{}", self.message))
                .build(),
        );
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ReducerError {
    ConversionError {
        value: SqliteValue,
        target_type: String,
    },

    Unknown(String),
}

impl Display for ReducerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ReducerError: {:?}", self)
    }
}

impl<E: Error> From<E> for ReducerError {
    fn from(e: E) -> Self {
        Self::Unknown(e.to_string())
    }
}

// conversion utilities for Row and SqliteValue

impl Row {
    pub fn get<'a, T>(&'a self, idx: usize) -> Result<T, ReducerError>
    where
        T: TryFrom<&'a SqliteValue, Error = ReducerError>,
    {
        T::try_from(self.get_value(idx))
    }

    pub fn maybe_get<'a, T>(&'a self, idx: usize) -> Result<Option<T>, ReducerError>
    where
        T: TryFrom<&'a SqliteValue, Error = ReducerError>,
    {
        match self.get_value(idx) {
            SqliteValue::Null => Ok(None),
            v => Ok(Some(T::try_from(v)?)),
        }
    }

    pub fn get_value(&self, idx: usize) -> &SqliteValue {
        self.0.get(idx).expect("row index out of bounds")
    }
}

macro_rules! impl_types_for_sqlvalue {
    ($e:path, $($t:ty),*) => {
        $(
            impl From<$t> for SqliteValue {
                fn from(t: $t) -> Self {
                    $e(t.into())
                }
            }

            impl TryFrom<&SqliteValue> for $t {
                type Error = ReducerError;

                fn try_from(value: &SqliteValue) -> Result<Self, Self::Error> {
                    match value {
                        $e(i) => Ok(*i as $t),
                        v => Err(ReducerError::ConversionError {
                            value: v.clone(),
                            target_type: stringify!($t).to_owned(),
                        }),
                    }
                }
            }
        )*
    };
}

impl_types_for_sqlvalue!(SqliteValue::Integer, i8, i16, i32, i64);
impl_types_for_sqlvalue!(SqliteValue::Real, f32, f64);

impl<T> From<Option<T>> for SqliteValue
where
    SqliteValue: From<T>,
{
    fn from(o: Option<T>) -> Self {
        match o {
            Some(t) => Self::from(t),
            None => Self::Null,
        }
    }
}

impl From<Vec<u8>> for SqliteValue {
    fn from(b: Vec<u8>) -> Self {
        Self::Blob(b)
    }
}

impl TryFrom<&SqliteValue> for Vec<u8> {
    type Error = ReducerError;

    fn try_from(value: &SqliteValue) -> Result<Self, Self::Error> {
        match value {
            SqliteValue::Blob(b) => Ok(b.clone()),
            v => Err(ReducerError::ConversionError {
                value: v.clone(),
                target_type: "Vec<u8>".to_owned(),
            }),
        }
    }
}

impl From<&str> for SqliteValue {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl<'a> TryFrom<&'a SqliteValue> for &'a str {
    type Error = ReducerError;

    fn try_from(value: &SqliteValue) -> Result<&str, Self::Error> {
        match value {
            SqliteValue::Text(s) => Ok(s.as_str()),
            v => Err(ReducerError::ConversionError {
                value: v.clone(),
                target_type: "&str".to_owned(),
            }),
        }
    }
}

impl From<String> for SqliteValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl TryFrom<&SqliteValue> for String {
    type Error = ReducerError;

    fn try_from(value: &SqliteValue) -> Result<Self, Self::Error> {
        match value {
            SqliteValue::Text(s) => Ok(s.clone()),
            v => Err(ReducerError::ConversionError {
                value: v.clone(),
                target_type: "String".to_owned(),
            }),
        }
    }
}

impl From<bool> for SqliteValue {
    fn from(b: bool) -> Self {
        Self::Integer(b as i64)
    }
}

impl TryFrom<&SqliteValue> for bool {
    type Error = ReducerError;

    fn try_from(value: &SqliteValue) -> Result<Self, Self::Error> {
        match value {
            SqliteValue::Integer(i) => Ok(*i != 0),
            v => Err(ReducerError::ConversionError {
                value: v.clone(),
                target_type: "bool".to_owned(),
            }),
        }
    }
}
