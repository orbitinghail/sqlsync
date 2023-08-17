// run this example with: `cargo wasi run --example host --features host`

use serde::{Deserialize, Serialize};
use sqlsync_reducer::host_ffi::{self, register_host_fns, FFIBufPtr, HostState, WasmExports};
use sqlsync_reducer::types::{ExecResponse, QueryResponse, ReducerError};
use wasmi::{Engine, Linker, Module, Store};
#[derive(Serialize, Deserialize)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

fn main() -> anyhow::Result<()> {
    // build guest.wasm using: `cargo build --target wasm32-unknown-unknown --example guest`
    let wasm_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/examples/guest.wasm");

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes[..])?;
    let mut linker = <Linker<HostState<i32>>>::new(&engine);

    register_host_fns(
        &mut linker,
        |_, msg| {
            println!("wasm_log: {}", msg);
        },
        |state, query| {
            println!("wasm_query: {:?}", query);
            *state += 1;
            println!("state is: {}", state);
            Ok(QueryResponse {
                columns: vec!["hello".to_string(), "world".to_string()],
                rows: vec![vec!["hello".to_string(), "world".to_string()]],
            })
        },
        |state, exec| {
            println!("wasm_exec: {:?}", exec);
            *state += 1;
            println!("state is: {}", state);
            Ok(ExecResponse { changes: 1 })
        },
    )?;

    let mut store = Store::new(&engine, HostState::new(123));
    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;

    // load instance exports, and save it into the store
    let exports = WasmExports::new(&store, &instance)?;
    store.data_mut().initialize(exports);

    // write mutation to the ffi_buffer
    let mutation = Mutation::Set("hello".to_string(), "world".to_string());
    let mutation_ptr = host_ffi::encode(store.data().exports(), &mut store, mutation)?;

    let reduce = instance.get_typed_func::<FFIBufPtr, FFIBufPtr>(&store, "reduce")?;
    let result = reduce.call(&mut store, mutation_ptr)?;

    if result != 0 {
        let err: ReducerError = host_ffi::decode(store.data().exports(), &mut store, result)?;
        anyhow::bail!(err);
    }

    Ok(())
}
