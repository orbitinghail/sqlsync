use std::{collections::HashMap, fmt::Debug};

use anyhow::anyhow;
use futures::{
    channel::mpsc::{self, UnboundedSender},
    SinkExt,
};
use serde::{Deserialize, Serialize};
use sqlsync::JournalId;
use tsify::{declare, Tsify};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::{
    doc_task::DocTask,
    net::ConnectionStatus,
    reactive::QueryKey,
    sql::SqlValue,
    utils::{fetch_reducer, WasmError, WasmResult},
};

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_INTERFACE: &'static str = r#"
import { JournalId } from "../../src/journal-id.ts";
import { PortRouter, PortId } from "../../src/port.ts";

export type HandlerId = number;
export type PageIdx = number;
export type QueryKey = string;

interface WorkerApi {
    handle(msg: HostToWorkerMsg): Promise<void>;
}
"#;

pub type PortId = u32;
pub type HandlerId = u32;

#[declare]
type DocId = JournalId;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "SendError", extends = js_sys::Error)]
    pub type PortSendErr;

    #[wasm_bindgen(method, js_name = "missingPorts")]
    pub fn missing_ports(this: &PortSendErr) -> Vec<PortId>;

    #[wasm_bindgen(typescript_type = "PortRouter")]
    #[derive(Debug, Clone)]
    pub type PortRouter;

    #[wasm_bindgen(method, js_name = "sendOne", catch)]
    pub fn send_one(
        this: &PortRouter,
        port: PortId,
        msg: WorkerToHostMsg,
    ) -> Result<(), PortSendErr>;

    #[wasm_bindgen(method, js_name = "sendMany", catch)]
    pub fn send_many(
        this: &PortRouter,
        port: Vec<PortId>,
        msg: WorkerToHostMsg,
    ) -> Result<(), PortSendErr>;

    #[wasm_bindgen(method, js_name = "sendAll")]
    pub fn send_all(this: &PortRouter, msg: WorkerToHostMsg);
}

#[derive(Debug, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(from_wasm_abi)]
pub struct HostToWorkerMsg {
    pub port_id: PortId,
    pub handler_id: HandlerId,
    pub doc_id: DocId,
    pub req: DocRequest,
}

impl HostToWorkerMsg {
    pub fn reply(&self, reply: DocReply) -> WorkerToHostMsg {
        WorkerToHostMsg::Reply { handler_id: self.handler_id, reply }
    }

    pub fn reply_err<E: Debug>(&self, err: E) -> WorkerToHostMsg {
        WorkerToHostMsg::Reply {
            handler_id: self.handler_id,
            reply: DocReply::Err { err: format!("{:?}", err) },
        }
    }
}

#[derive(Debug, Deserialize, Tsify)]
#[serde(tag = "tag", rename_all_fields = "camelCase")]
#[tsify(from_wasm_abi)]
pub enum DocRequest {
    Open {
        reducer_url: String,
    },
    Query {
        sql: String,
        params: Vec<SqlValue>,
    },
    QuerySubscribe {
        key: QueryKey,
        sql: String,
        params: Vec<SqlValue>,
    },
    QueryUnsubscribe {
        key: QueryKey,
    },
    Mutate {
        #[serde(with = "serde_bytes")]
        #[tsify(type = "Uint8Array")]
        mutation: Vec<u8>,
    },
    RefreshConnectionStatus,
    SetConnectionEnabled {
        enabled: bool,
    },
}

#[derive(Debug, Serialize, Tsify)]
#[serde(tag = "tag", rename_all_fields = "camelCase")]
#[tsify(into_wasm_abi)]
pub enum WorkerToHostMsg {
    Reply {
        handler_id: HandlerId,
        reply: DocReply,
    },
    Event {
        doc_id: DocId,
        evt: DocEvent,
    },
}

#[derive(Debug, Serialize, Tsify)]
#[serde(tag = "tag", rename_all_fields = "camelCase")]
#[tsify(into_wasm_abi)]
pub enum DocReply {
    Ack,
    RecordSet {
        columns: Vec<String>,
        rows: Vec<Vec<SqlValue>>,
    },
    Err {
        err: String,
    },
}

#[derive(Debug, Serialize, Tsify, Clone)]
#[serde(tag = "tag", rename_all_fields = "camelCase")]
#[tsify(into_wasm_abi)]
pub enum DocEvent {
    ConnectionStatus {
        status: ConnectionStatus,
    },
    SubscriptionChanged {
        key: QueryKey,
        columns: Vec<String>,
        rows: Vec<Vec<SqlValue>>,
    },
    SubscriptionErr {
        key: QueryKey,
        err: String,
    },
}

#[wasm_bindgen]
pub struct WorkerApi {
    coordinator_url: Option<String>,
    ports: PortRouter,
    inboxes: HashMap<DocId, UnboundedSender<HostToWorkerMsg>>,
}

#[wasm_bindgen]
impl WorkerApi {
    #[wasm_bindgen(constructor)]
    pub fn new(
        ports: PortRouter,
        coordinator_url: Option<String>,
    ) -> WorkerApi {
        WorkerApi { coordinator_url, ports, inboxes: HashMap::new() }
    }

    #[wasm_bindgen(skip_typescript)]
    pub async fn handle(&mut self, msg: JsValue) -> WasmResult<()> {
        let mut msg: HostToWorkerMsg = serde_wasm_bindgen::from_value(msg)?;
        log::info!("handle: {:?}", msg);

        match &msg.req {
            DocRequest::Open { reducer_url } => {
                if let Some(inbox) = self.inboxes.get_mut(&msg.doc_id) {
                    // doc is already open
                    // request a connection status update from the doc
                    msg.req = DocRequest::RefreshConnectionStatus;
                    inbox.send(msg).await?;
                } else {
                    // open the doc
                    self.spawn_doc_task(msg.doc_id, &reducer_url).await?;
                    let _ = self
                        .ports
                        .send_one(msg.port_id, msg.reply(DocReply::Ack));
                }
            }

            _ => match self.inboxes.get_mut(&msg.doc_id) {
                Some(inbox) => inbox.send(msg).await?,
                None => {
                    let _ = self.ports.send_one(
                        msg.port_id,
                        msg.reply_err(WasmError(anyhow!(
                            "no document with id {}",
                            msg.doc_id
                        ))),
                    );
                }
            },
        }

        Ok(())
    }

    async fn spawn_doc_task(
        &mut self,
        doc_id: JournalId,
        reducer_url: &str,
    ) -> Result<(), WasmError> {
        let (reducer, digest) = fetch_reducer(reducer_url).await?;

        let doc_url = self.coordinator_url.as_ref().map(|url| {
            format!(
                "{}/doc/{}?reducer={}",
                url,
                doc_id.to_base58(),
                bs58::encode(&digest).into_string()
            )
        });

        let (tx, rx) = mpsc::unbounded();

        let task =
            DocTask::new(doc_id, doc_url, reducer, rx, self.ports.clone())?;

        wasm_bindgen_futures::spawn_local(task.into_task());

        self.inboxes.insert(doc_id, tx);

        Ok(())
    }
}
