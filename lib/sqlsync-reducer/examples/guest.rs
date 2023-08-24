// build guest.wasm using: `cargo build --target wasm32-unknown-unknown --example guest`

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, query, types::ReducerError};

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = bincode::deserialize(&mutation)?;

    log::info!("received mutation: {:?}", mutation);

    log::info!("running query and execute at the same time");

    let x: Option<i64> = None;
    let query_future = query!("SELECT * FROM foo WHERE bar = ?", "baz", 1, 1.23, x);
    let exec_future = execute!("SELECT * FROM foo WHERE bar = ?", "baz");

    let (result, result2) = futures::join!(query_future, exec_future);

    log::info!("query result: {:?}", result);
    log::info!("exec result: {:?}", result2);

    log::info!("running another query");

    let query_future = query!("SELECT * FROM foo WHERE bar = ?", "baz");

    let result = execute!("SELECT * FROM foo WHERE bar = ?", "baz").await;
    log::info!("result: {:?}", result);

    let result = query_future.await;
    log::info!("final query result: {:?}", result);

    Ok(())
}

init_reducer!(reducer);
