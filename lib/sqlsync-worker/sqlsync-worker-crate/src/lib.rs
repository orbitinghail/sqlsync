mod utils;

use std::{cell::RefCell, convert::TryInto, io, rc::Rc};

use futures::{
    select,
    stream::{Fuse, SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use gloo::{
    net::websocket::{futures::WebSocket, Message},
    timers::future::{IntervalStream, TimeoutFuture},
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
use web_sys::console;

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
        // TODO: create a oneshot channel in order to shut down replication when the doc closes
        wasm_bindgen_futures::spawn_local(replication_task(doc.clone(), coordinator_url));
    }

    Ok(SqlSyncDocument { doc })
}

type DocCell = Rc<RefCell<LocalDocument<MemoryJournal>>>;
type WebsocketSplitPair = (SplitSink<WebSocket, Message>, Fuse<SplitStream<WebSocket>>);

async fn replication_task(doc: DocCell, coordinator_url: String) {
    loop {
        match replication_task_inner(doc.clone(), &coordinator_url).await {
            Ok(()) => {}
            Err(e) => {
                log::error!("replication error: {:?}", e);
                // restart after a delay
                TimeoutFuture::new(100).await;
            }
        }
    }
}

async fn replication_task_inner(doc: DocCell, coordinator_url: &str) -> WasmResult<()> {
    let doc_id = { doc.borrow().doc_id() };
    let url = format!("ws://{}/doc/{}", coordinator_url, doc_id.to_base58());

    let mut reconnect_timeout = 10;
    let mut sync_interval = IntervalStream::new(1000).fuse();
    let mut ws: Option<WebsocketSplitPair> = None;
    let mut protocol = ReplicationProtocol::new();

    loop {
        if let Some((ref mut writer, ref mut reader)) = ws {
            // now we need to select, either the sync timeout or the websocket
            select! {
                _ = sync_interval.next() => {
                    sync(&mut protocol, &doc, writer).await?;
                }
                msg = reader.select_next_some() => {
                    match msg {
                        Ok(msg) => {
                            // reset reconnect timeout on successful read
                            reconnect_timeout = 10;

                            handle_message(&mut protocol, &doc, writer, msg).await?;
                        }
                        Err(e) => {
                            log::error!("websocket error: {:?}", e);
                            // drop the websocket, we will reconnect on the next loop
                            ws = None;
                        }
                    }
                }
            }
        } else {
            // if we don't have a websocket, wait for the reconnect timeout
            TimeoutFuture::new(reconnect_timeout).await;

            // increase the exponential backoff
            reconnect_timeout *= 2;
            // with max
            reconnect_timeout = reconnect_timeout.min(10000);

            log::info!("connecting to {}", url);

            // open a new websocket
            // note: we don't know if this failed until we try to read
            let (mut writer, reader) = WebSocket::open(&url)?.split();

            // reset the protocol state
            protocol = ReplicationProtocol::new();

            // kickoff replication
            start_replication(&mut protocol, &doc, &mut writer).await?;

            ws = Some((writer, reader.fuse()));
        }
    }
}

async fn start_replication(
    protocol: &mut ReplicationProtocol,
    doc: &DocCell,
    writer: &mut SplitSink<WebSocket, Message>,
) -> WasmResult<()> {
    let start_msg = {
        let mut doc = doc.borrow_mut();
        protocol.start(&mut *doc)
    };
    log::info!("sending start message: {:?}", start_msg);
    let start_msg = bincode::serialize(&start_msg)?;
    writer.send(Message::Bytes(start_msg)).await?;

    Ok(())
}

async fn sync(
    protocol: &mut ReplicationProtocol,
    doc: &DocCell,
    writer: &mut SplitSink<WebSocket, Message>,
) -> WasmResult<()> {
    // send as many frames as we can
    loop {
        // we need to separate this block from the websocket write in order
        // to release the borrows while awaiting the write
        let msg_buf = {
            let mut doc = doc.borrow_mut();

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
            writer.send(Message::Bytes(buf.into_inner())).await?;
        } else {
            break;
        }
    }

    Ok(())
}

async fn handle_message(
    protocol: &mut ReplicationProtocol,
    doc: &DocCell,
    writer: &mut SplitSink<WebSocket, Message>,
    msg: Message,
) -> WasmResult<()> {
    match msg {
        Message::Bytes(bytes) => {
            // we need to separate this block from the websocket write in order
            // to release the borrows while awaiting the write
            let resp = {
                let mut doc = doc.borrow_mut();

                let mut buf = io::Cursor::new(bytes);
                let msg: ReplicationMsg = bincode::deserialize_from(&mut buf)?;
                log::info!("received message: {:?}", msg);

                let resp = protocol.handle(&mut *doc, msg, &mut buf)?;

                // for now we trigger rebase after every msg
                console::time_with_label("rebase");
                doc.rebase()?;
                console::time_end_with_label("rebase");

                resp
            };

            if let Some(resp) = resp {
                log::info!("sending response: {:?}", resp);
                let resp = bincode::serialize(&resp)?;
                writer.send(Message::Bytes(resp)).await?;
            }
        }
        Message::Text(_) => {
            return Err(anyhow::anyhow!("unexpected text message").into());
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
