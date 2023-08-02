mod utils;

use std::io;

use bincode::ErrorKind;
use log::Level;
use serde::{Deserialize, Serialize};
use sqlsync::{
    coordinator::CoordinatorDocument,
    local::LocalDocument,
    mutate::Mutator,
    positioned_io::{PositionedCursor, PositionedReader},
    Deserializable, Journal, MemoryJournal, RequestedLsnRange, Serializable, Syncable,
};
use utils::set_panic_hook;
use wasm_bindgen::prelude::*;
use web_sys::console;

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

static LOGGER: ConsoleLogger = ConsoleLogger;

#[derive(Clone, Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,
    Increment,
}

impl Serializable for Mutation {
    fn serialize_into<W: std::io::Write>(&self, writer: &mut W) -> io::Result<()> {
        match bincode::serialize_into(writer, &self) {
            Ok(_) => Ok(()),
            Err(err) => match err.as_ref() {
                ErrorKind::Io(err) => Err(err.kind().into()),
                _ => Err(io::Error::new(io::ErrorKind::Other, err)),
            },
        }
    }
}

impl Deserializable for Mutation {
    fn deserialize_from<R: PositionedReader>(reader: R) -> io::Result<Self> {
        match bincode::deserialize_from(PositionedCursor::new(reader)) {
            Ok(mutation) => Ok(mutation),
            Err(err) => match err.as_ref() {
                ErrorKind::Io(err) => Err(err.kind().into()),
                _ => Err(io::Error::new(io::ErrorKind::Other, err)),
            },
        }
    }
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

    let doc_id = 1;
    let m = MutatorImpl {};

    let client_id = 10;
    let storage = MemoryJournal::open(doc_id)?;
    let timeline = MemoryJournal::open(client_id)?;
    let mut local = LocalDocument::open(storage, timeline, m.clone())?;

    local.mutate(Mutation::InitSchema)?;
    local.mutate(Mutation::Increment)?;

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
    let storage_journal = MemoryJournal::open(doc_id)?;
    let mut remote = CoordinatorDocument::open(storage_journal, m.clone())?;

    // sync client -> server
    let req = RequestedLsnRange::new(0, 10);
    if let Some(partial) = local.sync_prepare(req)? {
        remote.sync_receive(partial.into_read_partial())?;
    }

    // step remote (apply changes)
    remote.step()?;

    // sync server -> client
    let req = RequestedLsnRange::new(0, 10);
    if let Some(partial) = remote.sync_prepare(req)? {
        local.sync_receive(partial.into_read_partial())?;
    }

    // run another local increment
    local.mutate(Mutation::Increment)?;

    // recheck the table
    local.query(|tx| {
        let mut stmt = tx.prepare("SELECT value, datetime('now'), random() FROM counter")?;
        let mut rows = stmt.query([])?;
        let row = rows.next()?.unwrap();
        let value: i64 = row.get(0)?;
        let date: String = row.get(1)?;
        let rand: i64 = row.get(2)?;
        assert_eq!(value, 2);
        log::info!("value: {}, date: {}, random: {}", value, date, rand);
        Ok(())
    })?;

    Ok(())
}
