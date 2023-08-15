// build guest.wasm using: `cargo build --target wasm32-unknown-unknown --example guest`

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{
    export_reducer,
    ffi::{fbm, log, query, FFIBufPtr},
    types::{Query, ReducerError},
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

fn reducer(mutation: Mutation) -> Result<(), ReducerError> {
    log(format!("received mutation: {:?}", mutation))?;
    log("running query".into())?;

    let result = query(Query {
        sql: "SELECT * FROM foo WHERE bar = ?".to_owned(),
        params: vec!["baz".to_owned()],
    })?;
    log(format!("query result: {:?}", result))?;

    Ok(())
}

export_reducer!(Mutation, reducer);
