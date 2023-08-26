use futures::stream::StreamExt;
use sqlsync::{coordinator::CoordinatorDocument, Journal, JournalId, MemoryJournal};
use worker::*;
use worker_sys::web_sys::Event;

#[durable_object]
pub struct DocumentCoordinator {
    state: State,
    env: Env,
    clients: Vec<ClientConnection>,
}

#[durable_object]
impl DurableObject for DocumentCoordinator {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            clients: Vec::new(),
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        if let Some(upgrade) = req.headers().get("Upgrade")? {
            if upgrade == "websocket" {
                let pair = WebSocketPair::new()?;
                let ws = pair.server;

                ws.accept()?;

                let client = ClientConnection::new(ws);
                client.spawn_event_handler();
                self.clients.push(client);

                return Response::from_websocket(pair.client);
            }
        }
        Response::error("Bad Request", 400)
    }
}

struct ClientConnection {
    ws: WebSocket,
}

impl ClientConnection {
    fn new(ws: WebSocket) -> Self {
        Self { ws }
    }

    fn spawn_event_handler(&self) {
        let ws = self.ws.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let mut events = ws.events().expect("failed to open event stream");

            while let Some(event) = events.next().await {
                match event.expect("failed to receive event") {
                    WebsocketEvent::Message(message) => {
                        let data = message.bytes().unwrap();
                        console_log!("received data with len {}", data.len());
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
        });
    }
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        .on_async("/doc/:id", |req, ctx| async move {
            if let Some(id) = ctx.param("id") {
                console_log!("forwarding request to document with id: {}", id);
                let namespace = ctx.durable_object("COORDINATOR")?;
                let stub = namespace.id_from_name(id)?.get_stub()?;
                stub.fetch_with_request(req).await
            } else {
                Response::error("Bad Request", 400)
            }
        })
        .run(req, env)
        .await
}
