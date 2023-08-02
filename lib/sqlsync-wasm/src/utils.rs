use log::Level;
use sqlsync::JournalError;
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
}

impl From<WasmError> for JsValue {
    fn from(value: WasmError) -> Self {
        todo!()
    }
}
