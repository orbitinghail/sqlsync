// run this example with: `cargo wasi run --example host --features host`

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{
    host_ffi::{register_log_handler, WasmFFI},
    types::{ExecResponse, QueryResponse, Request},
};
use wasmi::{Engine, Linker, Module, Store};

#[derive(Serialize, Deserialize)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Trace)
        .env()
        .init()?;

    // build guest.wasm using: `cargo build --target wasm32-unknown-unknown --example guest`
    let wasm_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/examples/guest.wasm");

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes[..])?;
    let mut linker = Linker::new(&engine);

    register_log_handler(&mut linker)?;

    let mut store = Store::new(&engine, WasmFFI::uninitialized());
    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;

    // initialize the FFI
    let ffi = WasmFFI::initialized(&store, &instance)?;
    (*store.data_mut()) = ffi.clone();

    // initialize the reducer
    ffi.init_reducer(&mut store)?;

    let mutation = Mutation::Set("hello".to_string(), "world".to_string());
    let mutation = &bincode::serialize(&mutation)?;

    // kick off the reducer
    let mut requests = ffi.reduce(&mut store, mutation)?;

    while let Some(requests_inner) = requests {
        // process requests
        let mut responses = BTreeMap::new();
        for (id, req) in requests_inner {
            match req {
                Request::Query { .. } => {
                    log::info!("received query request: {:?}", req);
                    let ptr = ffi.encode(
                        &mut store,
                        &QueryResponse {
                            columns: vec!["foo".into(), "bar".into()],
                            rows: vec![vec!["baz".into(), "qux".into()].into()],
                        },
                    )?;
                    responses.insert(id, ptr);
                }
                Request::Exec { .. } => {
                    log::info!("received exec request: {:?}", req);
                    let ptr = ffi.encode(&mut store, &ExecResponse { changes: 1 })?;
                    responses.insert(id, ptr);
                }
            }
        }

        // step the reactor forward
        requests = ffi.reactor_step(&mut store, Some(responses))?;
    }

    Ok(())
}
