use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct LogRequest {
    pub message: String,
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
