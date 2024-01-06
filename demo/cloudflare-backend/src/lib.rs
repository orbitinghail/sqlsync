use coordinator::Coordinator;
use js_sys::{ArrayBuffer, Reflect, Uint8Array};
use sqlsync::JournalId;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use worker::*;

mod coordinator;
mod persistence;

pub const DURABLE_OBJECT_NAME: &str = "COORDINATOR";
pub const REDUCER_BUCKET: &str = "SQLSYNC_REDUCERS";

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
        Self { state, env, coordinator: None }
    }

    async fn fetch(&mut self, req: Request) -> Result<Response> {
        // check that the Upgrade header is set and == "websocket"
        let is_upgrade_req = req.headers().get("Upgrade")?.unwrap_or("".into()) == "websocket";
        if !is_upgrade_req {
            return Response::error("Bad Request", 400);
        }

        // initialize the coordinator if it hasn't been initialized yet
        if self.coordinator.is_none() {
            // retrieve the reducer digest from the request url
            let url = req.url()?;
            let reducer_digest = match url.query_pairs().find(|(k, _)| k == "reducer") {
                Some((_, v)) => v,
                None => return Response::error("Bad Request", 400),
            };
            let bucket = self.env.bucket(REDUCER_BUCKET)?;
            let object = bucket
                .get(format!("{}.wasm", reducer_digest))
                .execute()
                .await?;
            let reducer_bytes = match object {
                Some(object) => {
                    object
                        .body()
                        .ok_or_else(|| Error::RustError("reducer not found in bucket".to_string()))?
                        .bytes()
                        .await?
                }
                None => {
                    return Response::error(
                        format!("reducer {} not found in bucket", reducer_digest),
                        404,
                    )
                }
            };

            let (coordinator, task) = Coordinator::init(&self.state, reducer_bytes).await?;
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
        .put_async("/reducer", |req, ctx| async move {
            // upload a reducer to the bucket
            let bucket = ctx.bucket(REDUCER_BUCKET)?;

            let data_len: u64 = match req.headers().get("Content-Length")?.map(|s| s.parse()) {
                Some(Ok(len)) => len,
                _ => return Response::error("Bad Request", 400),
            };
            if data_len > 10 * 1024 * 1024 {
                return Response::error("Payload Too Large", 413);
            }

            // let mut data = req.bytes().await?;
            let data = JsFuture::from(req.inner().array_buffer()?)
                .await?
                .dyn_into::<ArrayBuffer>()
                .expect("expected ArrayBuffer");

            let global = js_sys::global()
                .dyn_into::<js_sys::Object>()
                .expect("global not found");
            let subtle = Reflect::get(&global, &"crypto".into())?
                .dyn_into::<web_sys::Crypto>()
                .expect("crypto not found")
                .subtle();

            // sha256 sum the data and convert to bs58
            let digest =
                JsFuture::from(subtle.digest_with_str_and_buffer_source("SHA-256", &data)?).await?;

            // convert digest to base58
            let digest = bs58::encode(Uint8Array::new(&digest).to_vec())
                .with_alphabet(bs58::Alphabet::BITCOIN)
                .into_string();
            let name = format!("{}.wasm", digest);

            console_log!(
                "uploading reducer (size: {} MB) to {}",
                data_len / 1024 / 1024,
                name
            );

            // read data into Vec<u8>
            let data = Uint8Array::new(&data).to_vec();

            bucket.put(&name, data).execute().await?;
            Response::ok(name)
        })
        .on_async("/new", |_req, ctx| async move {
            let namespace = ctx.durable_object(DURABLE_OBJECT_NAME)?;
            let id = namespace.unique_id()?;
            let id = object_id_to_journal_id(id)?;
            console_log!("creating new document with id {}", id);
            Response::ok(id.to_base58())
        })
        .on_async("/new/:name", |_req, ctx| async move {
            if let Some(name) = ctx.param("name") {
                let namespace = ctx.durable_object(DURABLE_OBJECT_NAME)?;
                // until SQLSync is stable, named doc resolution will periodically break when we increment this counter
                let id = namespace.id_from_name(&format!("sqlsync-1-{}", name))?;
                let id = object_id_to_journal_id(id)?;
                Response::ok(id.to_base58())
            } else {
                Response::error("Bad Request", 400)
            }
        })
        .on_async("/doc/:id", |req, ctx| async move {
            if let Some(id) = ctx.param("id") {
                console_log!("forwarding request to document with id: {}", id);
                let namespace = ctx.durable_object(DURABLE_OBJECT_NAME)?;
                let id = JournalId::from_base58(id).map_err(|e| Error::RustError(e.to_string()))?;
                let id = match namespace.id_from_string(&id.to_hex()) {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::error(format!("Invalid Durable Object ID: {}", e), 400)
                    }
                };
                let stub = id.get_stub()?;
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
