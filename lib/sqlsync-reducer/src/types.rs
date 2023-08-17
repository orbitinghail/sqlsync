use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use log::Level;
use serde::{Deserialize, Serialize};

pub type RequestId = u32;

pub type Requests = Option<BTreeMap<RequestId, Request>>;
pub type Responses = Option<BTreeMap<RequestId, u32>>;

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    Query { sql: String, params: Vec<String> },
    Exec { sql: String, params: Vec<String> },
}

#[derive(Serialize, Deserialize)]
pub struct LogRecord {
    level: String,
    message: String,
    file: Option<String>,
    line: Option<u32>,
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
pub struct QueryRequest {
    pub sql: String,
    pub params: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExecRequest {
    pub sql: String,
    pub params: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExecResponse {
    pub changes: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ReducerError {
    BincodeError(String),
}

impl Display for ReducerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ReducerError: {:?}", self)
    }
}

impl Error for ReducerError {}

impl From<bincode::Error> for ReducerError {
    fn from(e: bincode::Error) -> Self {
        Self::BincodeError(e.to_string())
    }
}
