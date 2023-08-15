// run this example with: `cargo wasi run --example host --features host`

use serde::{Deserialize, Serialize};
use sqlsync_reducer::host_ffi::{self, FFIBufPtr, WasmExports};
use sqlsync_reducer::types::{LogParams, Query, QueryResult, ReducerError};
use wasmi::core::Trap;
use wasmi::{Caller, Engine, Linker, Module, Store};

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
    let mut linker = <Linker<Option<WasmExports>>>::new(&engine);

    linker.func_wrap(
        "env",
        "host_log",
        |mut caller: Caller<'_, Option<WasmExports>>, params: FFIBufPtr| -> Result<(), Trap> {
            let params: LogParams = host_ffi::decode(caller.data().unwrap(), &mut caller, params)
                .map_err(|e| Trap::new(e.to_string()))?;
            println!("wasm log: {}", params.message);
            Ok(())
        },
    )?;

    linker.func_wrap(
        "env",
        "host_query",
        |mut caller: Caller<'_, Option<WasmExports>>,
         query_ptr: FFIBufPtr|
         -> Result<FFIBufPtr, Trap> {
            // load the query from the ffi_buffer
            let exports = caller.data().unwrap();

            let query: Query = host_ffi::decode(exports, &mut caller, query_ptr)
                .map_err(|e| Trap::new(e.to_string()))?;

            println!("received query: {:?}", query);

            // send result
            let result = QueryResult {
                columns: vec!["hello".to_string(), "world".to_string()],
                rows: vec![vec!["hello".to_string(), "world".to_string()]],
            };
            println!("sending result: {:?}", result);

            host_ffi::encode(exports, &mut caller, result).map_err(|e| Trap::new(e.to_string()))
        },
    )?;

    let mut store = Store::new(&engine, None);
    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;

    // load instance exports, and save it into the store
    let exports = WasmExports::new(&store, &instance)?;
    store.data_mut().replace(exports);

    // write mutation to the ffi_buffer
    let mutation = Mutation::Set("hello".to_string(), "world".to_string());
    let mutation_ptr = host_ffi::encode(store.data().unwrap(), &mut store, mutation)?;

    let reduce = instance.get_typed_func::<FFIBufPtr, FFIBufPtr>(&store, "reduce")?;
    let result = reduce.call(&mut store, mutation_ptr)?;

    if result != 0 {
        let err: ReducerError = host_ffi::decode(store.data().unwrap(), &mut store, result)?;
        anyhow::bail!(err);
    }

    Ok(())
}
