use std::{cell::RefCell, io, rc::Rc, time::Duration};

use futures::{stream::StreamExt, Future};
use js_sys::Uint8Array;
use sqlsync::{
    coordinator::CoordinatorDocument,
    replication::{ReplicationMsg, ReplicationProtocol},
    Journal, JournalId, MemoryJournal,
};
use worker::*;

const DURABLE_OBJECT_NAME: &str = "COORDINATOR";
const REDUCER_BUCKET: &str = "SQLSYNC_REDUCERS";

// for now, we just support a fixed reducer; this key is used to grab the latest
// reducer from the bucket
const REDUCER_KEY: &str = "reducer.wasm";

type Document = Rc<RefCell<CoordinatorDocument<MemoryJournal>>>;

#[durable_object]
pub struct DocumentCoordinator {
    state: State,
    env: Env,
    doc: Option<Document>,
}

#[durable_object]
impl DurableObject for DocumentCoordinator {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();
        Self {
            state,
            env,
            doc: None,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        // check that the Upgrade header is set and == "websocket"
        let is_upgrade_req = req.headers().get("Upgrade")?.unwrap_or("".into()) == "websocket";
        if !is_upgrade_req {
            return Response::error("Bad Request", 400);
        }

        let doc_id = object_id_to_journal_id(self.state.id())?;

        if self.doc.is_none() {
            console_log!("creating new document with id {}", doc_id);

            let bucket = self.env.bucket(REDUCER_BUCKET)?;
            let object = bucket.get(REDUCER_KEY).execute().await?;
            let reducer_bytes = object
                .ok_or_else(|| Error::RustError("reducer not found".into()))?
                .body()
                .ok_or_else(|| Error::RustError("reducer not found".into()))?
                .bytes()
                .await?;

            let storage =
                MemoryJournal::open(doc_id).map_err(|e| Error::RustError(e.to_string()))?;
            let doc = Rc::new(RefCell::new(
                CoordinatorDocument::open(storage, &reducer_bytes)
                    .map_err(|e| Error::RustError(e.to_string()))?,
            ));

            // spawn a background task which steps the document
            spawn_local(coordinator_task(doc.clone()));

            self.doc = Some(doc);
        }
        let doc = self.doc.as_ref().unwrap();

        let pair = WebSocketPair::new()?;
        let ws = pair.server;

        ws.accept()?;

        spawn_client_tasks(ws, doc.clone())?;

        Response::from_websocket(pair.client)
    }
}

async fn coordinator_task(doc: Document) -> DynResult<()> {
    // TODO figure out how to signal the coordinator task using some kind of channel
    // so that we can wake it up after receiving mutations rather than running
    // it on a loop
    let step_interval = Duration::from_millis(1000);

    loop {
        {
            let mut doc = doc.borrow_mut();
            while doc.has_pending_work() {
                doc.step()?;
            }
        }

        // sleep for a bit
        Delay::from(step_interval).await
    }
}

#[derive(Clone)]
struct ClientState {
    ws: WebSocket,
    doc: Document,
    protocol: Rc<RefCell<ReplicationProtocol>>,
}

fn spawn_client_tasks(ws: WebSocket, doc: Document) -> Result<()> {
    let state = ClientState {
        ws: ws.clone(),
        doc: doc.clone(),
        protocol: Rc::new(RefCell::new(ReplicationProtocol::new())),
    };

    spawn_local(client_task_handle_sync(state.clone()));
    spawn_local(client_task_handle_messages(state.clone()));

    // kickoff replication
    let protocol = state.protocol.borrow_mut();
    let mut doc = state.doc.borrow_mut();
    let start_msg = protocol.start(&mut *doc);
    console_log!("sending start message: {:?}", start_msg);
    send_msg(&ws, start_msg)
}

async fn client_task_handle_sync(state: ClientState) -> DynResult<()> {
    let sync_interval = Duration::from_millis(1000);

    loop {
        {
            let mut protocol = state.protocol.borrow_mut();
            let mut doc = state.doc.borrow_mut();

            // send all outstanding frames
            while let Some((msg, mut reader)) = protocol.sync(&mut *doc)? {
                console_log!("sending message: {:?}", msg);
                let mut writer = io::Cursor::new(vec![]);

                bincode::serialize_into(&mut writer, &msg)?;
                io::copy(&mut reader, &mut writer)?;

                send_bytes(&state.ws, writer.into_inner().as_slice())?;
            }
        }

        // sleep for a bit
        Delay::from(sync_interval).await
    }
}

async fn client_task_handle_messages(state: ClientState) -> DynResult<()> {
    let mut events = state.ws.events()?;

    while let Some(event) = events.next().await {
        match event.expect("failed to receive event") {
            WebsocketEvent::Message(message) => {
                let data = message
                    .bytes()
                    .ok_or(Error::RustError("expected binary message".into()))?;

                let mut reader = io::Cursor::new(data);
                let msg = recv_msg(&mut reader)?;
                console_log!("received message: {:?}", msg);

                {
                    let mut protocol = state.protocol.borrow_mut();
                    let mut doc = state.doc.borrow_mut();

                    if let Some(resp) = protocol.handle(&mut *doc, msg, &mut reader)? {
                        console_log!("sending response: {:?}", resp);
                        send_msg(&state.ws, resp)?;
                    }
                }
            }
            WebsocketEvent::Close(evt) => {
                console_log!(
                    "websocket closed reason: {}, code: {}",
                    evt.reason(),
                    evt.code()
                );
            }
        }
    }

    Ok(())
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();
    let cors = Cors::default().with_origins(vec!["*"]);

    let router = Router::new();

    router
        .on_async("/new", |_req, ctx| async move {
            let namespace = ctx.durable_object(DURABLE_OBJECT_NAME)?;
            let id = namespace.unique_id()?;
            let id = object_id_to_journal_id(id)?;
            console_log!("creating new document with id {}", id);
            Response::ok(id.to_base58())
        })
        .on_async("/doc/:id", |req, ctx| async move {
            if let Some(id) = ctx.param("id") {
                console_log!("forwarding request to document with id: {}", id);
                let namespace = ctx.durable_object(DURABLE_OBJECT_NAME)?;
                let id =
                    JournalId::from_base58(&id).map_err(|e| Error::RustError(e.to_string()))?;
                let stub = namespace.id_from_string(&id.to_hex())?.get_stub()?;
                stub.fetch_with_request(req).await
            } else {
                Response::error("Bad Request", 400)
            }
        })
        .run(req, env)
        .await?
        .with_cors(&cors)
}

fn object_id_to_journal_id(id: ObjectId) -> Result<JournalId> {
    JournalId::from_hex(&id.to_string()).map_err(|e| e.to_string().into())
}

fn send_msg(ws: &WebSocket, msg: ReplicationMsg) -> Result<()> {
    let data = bincode::serialize(&msg).map_err(|e| Error::RustError(e.to_string()))?;
    send_bytes(ws, data.as_slice())
}

fn send_bytes(ws: &WebSocket, bytes: &[u8]) -> Result<()> {
    let uint8_array = Uint8Array::from(bytes);
    Ok(ws.as_ref().send_with_array_buffer(&uint8_array.buffer())?)
}

fn recv_msg(r: impl io::Read) -> Result<ReplicationMsg> {
    Ok(bincode::deserialize_from(r).map_err(|e| Error::RustError(e.to_string()))?)
}

type DynResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn spawn_local<F>(future: F)
where
    F: Future<Output = DynResult<()>> + 'static,
{
    wasm_bindgen_futures::spawn_local(async move {
        match future.await {
            Ok(()) => {}
            Err(e) => {
                console_error!("spawn_local error: {:?}", e);
            }
        }
    })
}
