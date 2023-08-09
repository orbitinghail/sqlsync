use std::io;

use js_sys::{Reflect, Uint8Array};
use log::Level;
use sqlsync::{
    sqlite::{
        self, params_from_iter,
        types::{ToSqlOutput, Value, ValueRef},
        ToSql, Transaction,
    },
    JournalError,
};
use thiserror::Error;
use wasm_bindgen::JsValue;
use web_sys::console;

pub fn set_panic_hook() {
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    console_error_panic_hook::set_once();
}

pub struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        let console_log = match record.level() {
            Level::Error => console::error_1,
            Level::Warn => console::warn_1,
            Level::Info => console::info_1,
            Level::Debug => console::log_1,
            Level::Trace => console::debug_1,
        };

        console_log(&format!("{}", record.args()).into());
    }

    fn flush(&self) {}
}

pub type WasmResult<T> = Result<T, WasmError>;

#[derive(Error, Debug)]
pub enum WasmError {
    #[error(transparent)]
    AnyhowError(#[from] anyhow::Error),

    #[error(transparent)]
    JournalError(#[from] JournalError),

    #[error(transparent)]
    SqliteError(#[from] sqlite::Error),

    #[error(transparent)]
    FromSqlError(#[from] sqlite::types::FromSqlError),

    #[error("JsValue error: {0:?}")]
    JsError(JsValue),
}

impl WasmError {
    pub fn into_anyhow(self) -> anyhow::Error {
        match self {
            Self::AnyhowError(e) => e,
            Self::JournalError(e) => e.into(),
            Self::SqliteError(e) => e.into(),
            Self::FromSqlError(e) => e.into(),
            Self::JsError(e) => anyhow::anyhow!("{:?}", e),
        }
    }
}

impl From<JsValue> for WasmError {
    fn from(value: JsValue) -> Self {
        Self::JsError(value)
    }
}

impl From<WasmError> for JsValue {
    fn from(value: WasmError) -> Self {
        match value {
            WasmError::AnyhowError(e) => JsValue::from_str(&format!("{:?}", e)),
            WasmError::JournalError(e) => JsValue::from_str(&format!("{:?}", e)),
            WasmError::SqliteError(e) => JsValue::from_str(&format!("{:?}", e)),
            WasmError::FromSqlError(e) => JsValue::from_str(&format!("{:?}", e)),
            WasmError::JsError(e) => e,
        }
    }
}

impl From<WasmError> for io::Error {
    fn from(value: WasmError) -> Self {
        match value {
            WasmError::AnyhowError(e) => io::Error::new(io::ErrorKind::Other, e),
            WasmError::JournalError(e) => io::Error::new(io::ErrorKind::Other, e),
            WasmError::SqliteError(e) => io::Error::new(io::ErrorKind::Other, e),
            WasmError::FromSqlError(e) => io::Error::new(io::ErrorKind::Other, e),
            WasmError::JsError(e) => io::Error::new(io::ErrorKind::Other, format!("{:?}", e)),
        }
    }
}

pub struct JsValueToSql<'a>(pub &'a JsValue);

impl<'a> ToSql for JsValueToSql<'a> {
    fn to_sql(&self) -> sqlsync::sqlite::Result<ToSqlOutput<'_>> {
        let js_type = self.0.js_typeof().as_string().unwrap();
        match js_type.as_str() {
            "undefined" => Ok(ToSqlOutput::Owned(Value::Null)),
            "null" => Ok(ToSqlOutput::Owned(Value::Null)),
            "boolean" => Ok(ToSqlOutput::Owned(self.0.as_bool().unwrap().into())),
            "number" => Ok(ToSqlOutput::Owned(self.0.as_f64().unwrap().into())),
            "string" => Ok(ToSqlOutput::Owned(self.0.as_string().unwrap().into())),
            _ => Err(sqlsync::sqlite::Error::ToSqlConversionFailure(
                format!("failed to convert from {}", js_type).into(),
            )),
        }
    }
}

pub struct JsValueFromSql<'a>(pub ValueRef<'a>);

impl<'a> From<JsValueFromSql<'a>> for JsValue {
    fn from(value: JsValueFromSql<'a>) -> Self {
        match value.0 {
            ValueRef::Null => JsValue::NULL,
            ValueRef::Integer(v) => JsValue::from(v),
            ValueRef::Real(v) => JsValue::from_f64(v),
            r @ ValueRef::Text(_) => r.as_str().unwrap().into(),
            ValueRef::Blob(v) => {
                let out = Uint8Array::new_with_length(v.len() as u32);
                out.copy_from(v);
                out.into()
            }
        }
    }
}
