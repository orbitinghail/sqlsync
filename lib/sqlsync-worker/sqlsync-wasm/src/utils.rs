use std::{convert::TryFrom, io};

use anyhow::anyhow;
use gloo::utils::errors::JsError;
use js_sys::Uint8Array;
use log::Level;
use sqlsync::sqlite::{
    self,
    types::{ToSqlOutput, Value, ValueRef},
    ToSql,
};
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

        console_log(&format!("sqlsync: {}", record.args()).into());
    }

    fn flush(&self) {}
}

pub type WasmResult<T> = Result<T, WasmError>;

#[derive(Debug)]
pub struct WasmError(anyhow::Error);

impl From<WasmError> for JsValue {
    fn from(value: WasmError) -> Self {
        JsValue::from_str(&value.0.to_string())
    }
}

impl From<JsValue> for WasmError {
    fn from(value: JsValue) -> Self {
        match JsError::try_from(value) {
            Ok(js_error) => WasmError(js_error.into()),
            Err(not_js_error) => WasmError(anyhow!(not_js_error.to_string())),
        }
    }
}

impl From<anyhow::Error> for WasmError {
    fn from(value: anyhow::Error) -> Self {
        WasmError(value)
    }
}

macro_rules! impl_from_error {
    ($($error:ty, )+) => {
        $(
            impl From<$error> for WasmError {
                fn from(value: $error) -> Self {
                    WasmError(anyhow::anyhow!(value))
                }
            }
        )+
    };
}

impl_from_error!(
    bincode::Error,
    io::Error,
    sqlsync::error::Error,
    sqlsync::sqlite::Error,
    sqlsync::JournalError,
    sqlsync::replication::ReplicationError,
    sqlsync::JournalIdParseError,
    gloo::utils::errors::JsError,
    gloo::net::websocket::WebSocketError,
);

pub struct JsValueToSql<'a>(pub &'a JsValue);

impl<'a> ToSql for JsValueToSql<'a> {
    fn to_sql(&self) -> sqlite::Result<ToSqlOutput<'_>> {
        let js_type = self.0.js_typeof().as_string().unwrap();
        match js_type.as_str() {
            "undefined" => Ok(ToSqlOutput::Owned(Value::Null)),
            "null" => Ok(ToSqlOutput::Owned(Value::Null)),
            "boolean" => Ok(ToSqlOutput::Owned(self.0.as_bool().unwrap().into())),
            "number" => Ok(ToSqlOutput::Owned(self.0.as_f64().unwrap().into())),
            "string" => Ok(ToSqlOutput::Owned(self.0.as_string().unwrap().into())),
            _ => Err(sqlite::Error::ToSqlConversionFailure(
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
