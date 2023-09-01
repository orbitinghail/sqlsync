mod utils;

use std::{cell::RefCell, convert::TryInto, io, rc::Rc};

use futures::{
    stream::{SplitSink, SplitStream},
    Future, SinkExt, StreamExt,
};
use gloo::{
    net::websocket::{futures::WebSocket, Message},
    timers::future::TimeoutFuture,
};
use js_sys::Reflect;
use sqlsync::{
    local::LocalDocument,
    replication::{ReplicationMsg, ReplicationProtocol},
    sqlite::params_from_iter,
    Journal, MemoryJournal,
};
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
    doc_id: &[u8],
    timeline_id: &[u8],
    reducer_wasm_bytes: &[u8],
    coordinator_url: Option<String>,
) -> WasmResult<SqlSyncDocument> {
    let storage = MemoryJournal::open(doc_id.try_into()?)?;
    let timeline = MemoryJournal::open(timeline_id.try_into()?)?;
    let doc = LocalDocument::open(storage, timeline, reducer_wasm_bytes)?;
    let doc = Rc::new(RefCell::new(doc));

    if let Some(coordinator_url) = coordinator_url {
        spawn_replication_tasks(&coordinator_url, doc.clone())?;
    }

    Ok(SqlSyncDocument { doc })
}

fn spawn_replication_tasks(
    coordinator_url: &str,
    doc: Rc<RefCell<LocalDocument<MemoryJournal>>>,
) -> WasmResult<()> {
    let doc_id = { doc.borrow().doc_id() };
    let url = format!("ws://{}/doc/{}", coordinator_url, doc_id.to_base58());
    let ws = WebSocket::open(&url)?;
    let (writer, reader) = ws.split();
    let protocol = Rc::new(RefCell::new(ReplicationProtocol::new()));
    let writer = Rc::new(RefCell::new(writer));

    let state = ReplicationTaskState {
        doc: doc.clone(),
        protocol: protocol.clone(),
    };
    spawn_fallible_task(replication_sync_task(state.clone(), writer.clone()));
    spawn_fallible_task(replication_read_task(state.clone(), reader, writer.clone()));

    Ok(())
}

#[derive(Clone)]
struct ReplicationTaskState {
    doc: Rc<RefCell<LocalDocument<MemoryJournal>>>,
    protocol: Rc<RefCell<ReplicationProtocol>>,
}

async fn replication_sync_task(
    state: ReplicationTaskState,
    writer: Rc<RefCell<SplitSink<WebSocket, Message>>>,
) -> WasmResult<()> {
    let sync_interval = 1000;

    loop {
        // keep sending while we have things to send
        loop {
            // we need to separate this block from the websocket write in order
            // to release the borrows while awaiting the write
            let msg_buf = {
                let mut protocol = state.protocol.borrow_mut();
                let mut doc = state.doc.borrow_mut();

                // read an outstanding frame into the msg_buf
                if let Some((msg, mut reader)) = protocol.sync(&mut *doc)? {
                    log::info!("sending message: {:?}", msg);

                    let mut buf = io::Cursor::new(vec![]);
                    bincode::serialize_into(&mut buf, &msg)?;
                    io::copy(&mut reader, &mut buf)?;
                    Some(buf)
                } else {
                    None
                }
            };

            if let Some(buf) = msg_buf {
                let mut writer = writer.borrow_mut();
                writer.send(Message::Bytes(buf.into_inner())).await?;
            } else {
                break;
            }
        }

        // sleep for a bit
        TimeoutFuture::new(sync_interval).await;
    }
}

async fn replication_read_task(
    state: ReplicationTaskState,
    mut reader: SplitStream<WebSocket>,
    writer: Rc<RefCell<SplitSink<WebSocket, Message>>>,
) -> WasmResult<()> {
    // kickoff replication
    let start_msg = {
        let protocol = state.protocol.borrow_mut();
        let mut doc = state.doc.borrow_mut();
        protocol.start(&mut *doc)
    };
    log::info!("sending start message: {:?}", start_msg);
    let start_msg = bincode::serialize(&start_msg)?;
    writer.borrow_mut().send(Message::Bytes(start_msg)).await?;

    while let Some(msg) = reader.next().await {
        match msg {
            Ok(Message::Bytes(bytes)) => {
                // we need to separate this block from the websocket write in order
                // to release the borrows while awaiting the write
                let resp = {
                    let mut protocol = state.protocol.borrow_mut();
                    let mut doc = state.doc.borrow_mut();

                    let mut buf = io::Cursor::new(bytes);
                    let msg: ReplicationMsg = bincode::deserialize_from(&mut buf)?;
                    log::info!("received message: {:?}", msg);

                    let resp = protocol.handle(&mut *doc, msg, &mut buf)?;

                    // for now we trigger rebase after every message
                    doc.rebase()?;

                    resp
                };

                if let Some(resp) = resp {
                    log::info!("sending response: {:?}", resp);
                    let resp = bincode::serialize(&resp)?;
                    writer.borrow_mut().send(Message::Bytes(resp)).await?;
                }
            }
            Ok(Message::Text(_)) => {
                return Err(anyhow::anyhow!("unexpected text message").into());
            }
            Err(e) => {
                log::error!("websocket read error: {:?}", e);
                return Err(e.into());
            }
        }
    }

    Ok(())
}

#[wasm_bindgen]
pub struct SqlSyncDocument {
    doc: Rc<RefCell<LocalDocument<MemoryJournal>>>,
}

#[wasm_bindgen]
impl SqlSyncDocument {
    pub fn mutate(&mut self, mutation: &[u8]) -> WasmResult<()> {
        Ok(self.doc.borrow_mut().mutate(mutation)?)
    }

    // defined in typescript_custom_section for better param and result types
    #[wasm_bindgen(skip_typescript)]
    pub fn query(&mut self, sql: String, params: Vec<JsValue>) -> WasmResult<Vec<js_sys::Object>> {
        Ok(self.doc.borrow_mut().query(|tx| {
            let params = params_from_iter(params.iter().map(|v| JsValueToSql(v)));
            let mut stmt = tx.prepare(&sql)?;

            let column_names: Vec<_> = stmt.column_names().iter().map(|&s| s.to_owned()).collect();

            let obj = stmt
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
                .collect::<Result<Vec<_>, _>>();
            obj
        })?)
    }
}

fn spawn_fallible_task<F>(future: F)
where
    F: Future<Output = WasmResult<()>> + 'static,
{
    wasm_bindgen_futures::spawn_local(async move {
        match future.await {
            Ok(()) => {}
            Err(e) => {
                log::error!("spawn_local error: {:?}", e);
            }
        }
    })
}
