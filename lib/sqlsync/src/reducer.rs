use std::any::Any;

use rusqlite::{params, Connection, Transaction};
use sqlsync_reducer::{
    host_ffi::{register_host_fns, HostState, WasmExports},
    types::{ExecResponse, QueryResponse},
};
use wasmi::{Engine, Linker, Module, Store};

struct Reducer<'a> {
    engine: Engine,
    module: Module,
    linker: Linker<HostState<&'a mut Connection>>,
}

impl Reducer<'_> {
    fn new() -> anyhow::Result<Self> {
        let wasm_bytes =
            include_bytes!("../../../target/wasm32-unknown-unknown/debug/examples/guest.wasm");

        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes[..])?;

        let mut linker: Linker<HostState<&mut Connection>> = Linker::new(&engine);

        register_host_fns(
            &mut linker,
            |_, msg| {
                log::info!("reducer log: {}", msg);
            },
            |state, query| {
                println!("wasm_query: {:?}", query);
                state.execute(query.sql.as_str(), params![])?;
                Ok(QueryResponse {
                    columns: vec!["hello".to_string(), "world".to_string()],
                    rows: vec![vec!["hello".to_string(), "world".to_string()]],
                })
            },
            |state, exec| {
                println!("wasm_exec: {:?}", exec);
                Ok(ExecResponse { changes: 1 })
            },
        )?;

        Ok(Self {
            engine,
            module,
            linker,
        })
    }

    fn apply(&self, tx: &mut Transaction, mutation: &[u8]) -> anyhow::Result<()> {
        // one day we'll figure out how to hoist this into Reducer, but for now,
        // not gonna battle with lifetimes
        let mut linker = Linker::<HostState<Transaction>>::new(&self.engine);

        register_host_fns(
            &mut linker,
            |_, msg| {
                log::info!("reducer log: {}", msg);
            },
            |state, query| {
                println!("wasm_query: {:?}", query);
                state.execute(query.sql.as_str(), params![])?;
                Ok(QueryResponse {
                    columns: vec!["hello".to_string(), "world".to_string()],
                    rows: vec![vec!["hello".to_string(), "world".to_string()]],
                })
            },
            |state, exec| {
                println!("wasm_exec: {:?}", exec);
                Ok(ExecResponse { changes: 1 })
            },
        )?;

        let mut store = Store::new(&self.engine, HostState::new(*tx));
        let instance = linker
            .instantiate(&mut store, &self.module)?
            .start(&mut store)?;

        let exports = WasmExports::new(&store, &instance)?;
        store.data_mut().initialize(exports.clone());

        // call reduce
        exports.reduce(&mut store, mutation)?;

        Ok(())
    }
}
