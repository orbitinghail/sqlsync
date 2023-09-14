mod api;
mod doc_task;
mod net;
mod reactive;
mod signal;
mod sql;
mod utils;

use utils::ConsoleLogger;
use wasm_bindgen::prelude::wasm_bindgen;

static LOGGER: ConsoleLogger = ConsoleLogger;

#[wasm_bindgen(start)]
pub fn main() {
    utils::set_panic_hook();
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}
