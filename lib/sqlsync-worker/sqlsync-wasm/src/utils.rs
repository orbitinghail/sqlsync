use std::{convert::TryFrom, fmt::Display, io};

use anyhow::anyhow;
use gloo::{
    net::http::Request, timers::future::TimeoutFuture, utils::errors::JsError,
};
use js_sys::{Reflect, Uint8Array};
use log::Level;
use sha2::{Digest, Sha256};
use sqlsync::Reducer;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
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
pub struct WasmError(pub anyhow::Error);

impl Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
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

impl From<WasmError> for JsValue {
    fn from(value: WasmError) -> Self {
        JsValue::from_str(&format!("{}", value))
    }
}

impl From<anyhow::Error> for WasmError {
    fn from(value: anyhow::Error) -> Self {
        WasmError(value)
    }
}

impl From<serde_wasm_bindgen::Error> for WasmError {
    fn from(value: serde_wasm_bindgen::Error) -> Self {
        WasmError(anyhow!(value.to_string()))
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
    sqlsync::ReducerError,
    gloo::utils::errors::JsError,
    gloo::net::Error,
    gloo::net::websocket::WebSocketError,
    futures::channel::mpsc::SendError,
);

pub async fn fetch_reducer(
    reducer_url: &str,
) -> Result<(Reducer, Vec<u8>), WasmError> {
    let resp = Request::get(reducer_url).send().await?;
    if !resp.ok() {
        return Err(WasmError(anyhow!(
            "failed to load reducer; response has status: {} {}",
            resp.status(),
            resp.status_text()
        )));
    }

    let mut reducer_wasm_bytes = resp.binary().await?;

    let global = js_sys::global()
        .dyn_into::<js_sys::Object>()
        .expect("global not found");
    let subtle = Reflect::get(&global, &"crypto".into())?
        .dyn_into::<web_sys::Crypto>()
        .expect("crypto not found")
        .subtle();

    let digest: Vec<u8> = if subtle.is_undefined() {
        let mut hasher = Sha256::new();
        hasher.update(&reducer_wasm_bytes);
        hasher.finalize().to_vec()
    } else {
        // sha256 sum the data
        // TODO: it would be much better to stream the data through the hash function
        // but afaik that's not doable with the crypto.subtle api
        let digest = JsFuture::from(subtle.digest_with_str_and_u8_array(
            "SHA-256",
            &mut reducer_wasm_bytes,
        )?)
        .await?;
        Uint8Array::new(&digest).to_vec()
    };

    let reducer = Reducer::new(reducer_wasm_bytes.as_slice())?;

    Ok((reducer, digest))
}

pub struct Backoff {
    current_ms: u32,
    max_ms: u32,
    future: Option<TimeoutFuture>,
}

impl Backoff {
    pub fn new(start_ms: u32, max_ms: u32) -> Self {
        Self { current_ms: start_ms, max_ms, future: None }
    }

    /// increase the backoff time if needed
    pub fn step(&mut self) {
        self.current_ms *= 2;
        self.current_ms = self.current_ms.min(self.max_ms);
        self.future = None;
    }

    /// block until the current backoff time has elapsed
    pub async fn wait(&mut self) {
        let current_ms = self.current_ms;
        self.future
            .get_or_insert_with(|| TimeoutFuture::new(current_ms))
            .await;
    }
}
