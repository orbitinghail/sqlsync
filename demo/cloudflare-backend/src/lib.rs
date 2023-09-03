use coordinator::Coordinator;
use sqlsync::JournalId;
use wasm_bindgen_futures::spawn_local;
use worker::*;

mod coordinator;
mod persistence;

pub const DURABLE_OBJECT_NAME: &str = "COORDINATOR";
pub const REDUCER_BUCKET: &str = "SQLSYNC_REDUCERS";

// for now, we just support a fixed reducer; this key is used to grab the latest
// reducer from the bucket
pub const REDUCER_KEY: &str = "reducer.wasm";

#[durable_object]
pub struct DocumentCoordinator {
    state: State,
    env: Env,
    coordinator: Option<Coordinator>,
}

#[durable_object]
impl DurableObject for DocumentCoordinator {
    fn new(state: State, env: Env) -> Self {
        console_error_panic_hook::set_once();
        Self {
            state,
            env,
            coordinator: None,
        }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        // check that the Upgrade header is set and == "websocket"
        let is_upgrade_req = req.headers().get("Upgrade")?.unwrap_or("".into()) == "websocket";
        if !is_upgrade_req {
            return Response::error("Bad Request", 400);
        }

        // initialize the coordinator if it hasn't been initialized yet
        if self.coordinator.is_none() {
            let (coordinator, task) = Coordinator::init(&self.state, &self.env).await?;
            spawn_local(task.into_task());
            self.coordinator = Some(coordinator);
        }
        let coordinator = self.coordinator.as_mut().unwrap();

        let pair = WebSocketPair::new()?;
        let ws = pair.server;
        ws.accept()?;

        if let Err(e) = coordinator
            .accept(ws.as_ref().clone().try_into().unwrap())
            .await
        {
            // the only case we get an error here is if the coordinator task has
            // somehow crashed and thus the Sender is disconnected
            panic!("failed to accept websocket: {:?}", e);
        }

        Response::from_websocket(pair.client)
    }
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();
    let cors = Cors::default().with_origins(vec!["*"]);

    let router = Router::new();

    router
        .put_async("/reducer", |mut req, ctx| async move {
            // upload a reducer to the bucket
            // this just overwrites the existing reducer
            let bucket = ctx.bucket(REDUCER_BUCKET)?;
            let data = req.bytes().await?;
            bucket.put(REDUCER_KEY, data).execute().await?;
            Response::ok("ok")
        })
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

pub fn object_id_to_journal_id(id: ObjectId) -> Result<JournalId> {
    JournalId::from_hex(&id.to_string()).map_err(|e| e.to_string().into())
}
