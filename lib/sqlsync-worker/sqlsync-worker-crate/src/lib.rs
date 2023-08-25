mod utils;

use std::convert::TryInto;

use js_sys::Reflect;
use sqlsync::{local::LocalDocument, sqlite::params_from_iter, Journal, MemoryJournal};
use utils::{ConsoleLogger, JsValueFromSql, JsValueToSql, WasmError, WasmResult};
use wasm_bindgen::prelude::*;

static LOGGER: ConsoleLogger = ConsoleLogger;

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_INTERFACE: &'static str = r#"
export type SqlValue = undefined | null | boolean | number | string;
export type Row = { [key: string]: SqlValue };

interface SqlSyncDocument {
  query(sql: string, params: SqlValue[]): Row[];
  query<T>(sql: string, params: SqlValue[]): T[];
}
"#;

#[wasm_bindgen(start)]
pub fn main() {
    utils::set_panic_hook();
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

#[wasm_bindgen]
pub fn open(
    doc_id: String,
    timeline_id: String,
    reducer_wasm_bytes: &[u8],
) -> WasmResult<SqlSyncDocument> {
    let storage = MemoryJournal::open(doc_id.try_into()?)?;
    let timeline = MemoryJournal::open(timeline_id.try_into()?)?;

    Ok(SqlSyncDocument {
        doc: LocalDocument::open(storage, timeline, reducer_wasm_bytes)?,
    })
}

#[wasm_bindgen]
pub struct SqlSyncDocument {
    doc: LocalDocument<MemoryJournal>,
}

#[wasm_bindgen]
impl SqlSyncDocument {
    pub fn mutate(&mut self, mutation: &[u8]) -> WasmResult<()> {
        Ok(self.doc.mutate(mutation)?)
    }

    // defined in typescript_custom_section for better param and result types
    #[wasm_bindgen(skip_typescript)]
    pub fn query(&mut self, sql: String, params: Vec<JsValue>) -> WasmResult<Vec<js_sys::Object>> {
        Ok(self.doc.query(|tx| {
            let params = params_from_iter(params.iter().map(|v| JsValueToSql(v)));
            let mut stmt = tx.prepare(&sql)?;

            let column_names: Vec<_> = stmt.column_names().iter().map(|&s| s.to_owned()).collect();

            let out = stmt
                .query_and_then(params, move |row| {
                    let row_obj = js_sys::Object::new();
                    for (i, column_name) in column_names.iter().enumerate() {
                        Reflect::set(
                            &row_obj,
                            &column_name.into(),
                            &JsValueFromSql(row.get_ref(i)?).into(),
                        )?;
                    }
                    Ok::<_, WasmError>(row_obj)
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.into_anyhow());
            out
        })?)
    }
}
