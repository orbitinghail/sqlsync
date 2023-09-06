mod utils;

use std::{cell::RefCell, convert::TryInto, io, rc::Rc};

use futures::{
    channel::mpsc::{channel, Receiver},
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
    MemoryJournal,
};
use utils::{ConsoleLogger, JsValueFromSql, JsValueToSql, WasmError, WasmResult};
use wasm_bindgen::prelude::*;
use web_sys::{console, EventTarget};

static LOGGER: ConsoleLogger = ConsoleLogger;

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_INTERFACE: &'static str = r#"
export type SqlValue = undefined | null | boolean | number | string;
export type Row = { [key: string]: SqlValue };

interface SqlSyncDocument {
  query<T extends Row>(sql: string, params: SqlValue[]): T[];
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
    reducer_digest: &[u8],
    coordinator_url: Option<String>,
    event_target: EventTarget,
) -> WasmResult<SqlSyncDocument> {
    let storage = MemoryJournal::open(doc_id.try_into()?)?;
    let timeline = MemoryJournal::open(timeline_id.try_into()?)?;
    let mut doc = LocalDocument::open(storage, timeline, reducer_wasm_bytes)?;

    let (mut changes_tx, changes_rx) = channel(10);

    /*

       browser toggle connection state
           notify replicaton to change connecton state

    */

    // TODO: Build a more robust (and granular) query subscription system
    let evt_tgt = event_target.clone();
    doc.subscribe(move || {
        let evt = &web_sys::Event::new("change").unwrap();
        evt_tgt.dispatch_event(evt).unwrap();
        let _ = changes_tx.try_send(0);
    });

    let doc = Rc::new(RefCell::new(doc));
    let replication_status = Rc::new(RefCell::new(ReplicationStatus {
        enabled: true,
        connected: false,
        notify: Box::new(move |connected| {
            let evt = &web_sys::CustomEvent::new_with_event_init_dict(
                "connected",
                web_sys::CustomEventInit::new().detail(&JsValue::from_bool(connected)),
            )
            .unwrap();
            event_target.dispatch_event(evt).unwrap();
        }),
    }));

    if let Some(coordinator_url) = coordinator_url {
        // TODO: create a oneshot channel in order to shut down replication when the doc closes
        wasm_bindgen_futures::spawn_local(replication_task(
            replication_status.clone(),
            doc.clone(),
            coordinator_url,
            reducer_digest.to_owned(),
            changes_rx,
        ));
    }

    Ok(SqlSyncDocument {
        doc,
        replication_status,
    })
}

type DocCell = Rc<RefCell<LocalDocument<MemoryJournal>>>;
type WebsocketSplitPair = (SplitSink<WebSocket, Message>, Fuse<SplitStream<WebSocket>>);

struct ReplicationStatus {
    /// enabled is controlled by the app, if it's false we will not attempt to reconnect
    enabled: bool,
    /// connected is set by the replication task
    connected: bool,
    /// notify is called by the replication task when the connection status changes
    notify: Box<dyn FnMut(bool)>,
}

async fn replication_task(
    status: Rc<RefCell<ReplicationStatus>>,
    doc: DocCell,
    coordinator_url: String,
    reducer_digest: Vec<u8>,
    mut doc_changed: Receiver<u8>,
) {
    loop {
        match replication_task_inner(
            status.clone(),
            doc.clone(),
            &coordinator_url,
            &reducer_digest,
            &mut doc_changed,
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                log::error!("replication error: {:?}", e);
                // restart after a delay
                TimeoutFuture::new(100).await;
            }
        }
    }
}

async fn replication_task_inner(
    status: Rc<RefCell<ReplicationStatus>>,
    doc: DocCell,
    coordinator_url: &str,
    reducer_digest: &[u8],
    doc_changed: &mut Receiver<u8>,
) -> WasmResult<()> {
    let doc_id = { doc.borrow().doc_id() };
    let reducer_digest_b58 = bs58::encode(reducer_digest).into_string();

    // use ws if the coordinator url contains "localhost"
    let proto = if coordinator_url.contains("localhost") {
        "ws"
    } else {
        "wss"
    };

    let url = format!(
        "{}://{}/doc/{}?reducer={}",
        proto,
        coordinator_url,
        doc_id.to_base58(),
        reducer_digest_b58
    );

    let mut reconnect_timeout_ms = 10;
    let mut sync_interval = IntervalStream::new(1000).fuse();
    let mut ws: Option<WebsocketSplitPair> = None;
    let mut protocol = ReplicationProtocol::new();

    loop {
        if !status.borrow().enabled {
            // drop the websocket
            ws = None;
        }

        if let Some((ref mut writer, ref mut reader)) = ws {
            // now we need to select, either the sync timeout or the websocket
            select! {
                _ = doc_changed.select_next_some() => {
                    sync(&mut protocol, &doc, writer).await?;
                }
                _ = sync_interval.next() => {
                    sync(&mut protocol, &doc, writer).await?;
                }
                msg = reader.select_next_some() => {
                    match msg {
                        Ok(msg) => {
                            if !status.borrow().connected {
                                log::info!("connected to {}", url);
                                let mut status = status.borrow_mut();
                                // mark connected
                                status.connected = true;
                                // notify connected
                                (status.notify)(true);
                                // reset reconnect timeout
                                reconnect_timeout_ms = 10;
                            }

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
            // set the connection status
            {
                let mut status = status.borrow_mut();
                if status.connected {
                    log::info!("disconnected from {}", url);
                    // mark disconnected
                    status.connected = false;
                    // notify disconnected
                    (status.notify)(false);
                }
            }

            // if we don't have a websocket, wait for the reconnect timeout
            TimeoutFuture::new(reconnect_timeout_ms).await;

            // increase the exponential backoff up to a max of 2 seconds
            reconnect_timeout_ms *= 2;
            reconnect_timeout_ms = reconnect_timeout_ms.min(2000);

            if !status.borrow().enabled {
                // if replication is disabled, we don't need to reconnect
                continue;
            }

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
    doc: DocCell,
    replication_status: Rc<RefCell<ReplicationStatus>>,
}

#[wasm_bindgen]
impl SqlSyncDocument {
    pub fn mutate(&mut self, mutation: &[u8]) -> WasmResult<()> {
        Ok(self.doc.borrow_mut().mutate(mutation)?)
    }

    pub fn is_connected(&self) -> bool {
        self.replication_status.borrow().connected
    }

    pub fn set_replication_enabled(&self, enabled: bool) {
        self.replication_status.borrow_mut().enabled = enabled;
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
