mod utils;

use log::Level;
use sqlsync::{unixtime::UnixTime, Mutator};
use utils::set_panic_hook;
use wasm_bindgen::prelude::*;
use web_sys::console;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

struct ConsoleLogger;

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

#[derive(Clone)]
struct WasmUnixTime;

impl UnixTime for WasmUnixTime {
    fn unix_timestamp(&self) -> i64 {
        js_sys::Date::now() as i64
    }
}

static LOGGER: ConsoleLogger = ConsoleLogger;

#[derive(Clone)]
enum Mutation {
    InitSchema,
    Increment,
}

#[derive(Clone)]
struct MutatorImpl {}
impl Mutator for MutatorImpl {
    type Mutation = Mutation;

    fn apply(
        &self,
        tx: &mut sqlsync::Transaction,
        mutation: &Self::Mutation,
    ) -> anyhow::Result<()> {
        match mutation {
            Mutation::InitSchema => tx.execute_batch(
                "
                    CREATE TABLE IF NOT EXISTS counter (
                        value INTEGER PRIMARY KEY
                    );
                    INSERT INTO counter (value) VALUES (0);
                ",
            )?,

            Mutation::Increment => tx.execute_batch("UPDATE counter SET value = value + 1")?,
        }
        Ok(())
    }
}

#[wasm_bindgen]
pub fn run() -> Result<(), JsValue> {
    run_inner().map_err(|e| JsValue::from_str(&e.to_string()))
}

pub fn run_inner() -> anyhow::Result<()> {
    set_panic_hook();

    log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug))?;

    let local_id = 1;
    let mut local = sqlsync::Local::new(local_id, MutatorImpl {}, WasmUnixTime {});

    local.run(Mutation::InitSchema)?;
    local.run(Mutation::Increment)?;

    local.query(|tx| {
        let mut stmt = tx.prepare("SELECT value, datetime('now'), random() FROM counter")?;
        let mut rows = stmt.query([])?;
        let row = rows.next()?.unwrap();
        let value: i64 = row.get(0)?;
        let date: String = row.get(1)?;
        let rand: i64 = row.get(2)?;
        assert_eq!(value, 1);
        log::info!("value: {}, date: {}, random: {}", value, date, rand);
        Ok(())
    })?;

    // try syncing to server, running a step, and then syncing back
    let mut remote = sqlsync::Remote::new(MutatorImpl {}, WasmUnixTime {});

    // sync client -> server
    let mut req = local.sync_timeline_prepare();
    if let Some(req) = req.take() {
        let resp = remote.handle_client_sync_timeline(local_id, req)?;
        local.sync_timeline_response(resp);
    }

    // step remote (apply changes)
    remote.step()?;

    // sync server -> client
    let req = local.sync_storage_request();
    let resp = remote.handle_client_sync_storage(req);
    if let Some(resp) = resp {
        local.sync_storage_receive(resp)?;
    }

    // recheck the table
    local.query(|tx| {
        let mut stmt = tx.prepare("SELECT value, datetime('now'), random() FROM counter")?;
        let mut rows = stmt.query([])?;
        let row = rows.next()?.unwrap();
        let value: i64 = row.get(0)?;
        let date: String = row.get(1)?;
        let rand: i64 = row.get(2)?;
        assert_eq!(value, 1);
        log::info!("value: {}, date: {}, random: {}", value, date, rand);
        Ok(())
    })?;

    Ok(())
}
