// build guest.wasm using: `cargo build --target wasm32-unknown-unknown --example guest`

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{
    guest_reactor::{execute, query},
    init_reducer,
    types::ReducerError,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

async fn reducer(mutation: Mutation) -> Result<(), ReducerError> {
    log::info!("received mutation: {:?}", mutation);

    log::info!("running query and execute at the same time");

    let query_future = query(
        "SELECT * FROM foo WHERE bar = ?".to_owned(),
        vec!["baz".to_owned()],
    );
    let exec_future = execute(
        "SELECT * FROM foo WHERE bar = ?".to_owned(),
        vec!["baz".to_owned()],
    );
    let (result, result2) = futures::join!(query_future, exec_future);

    log::info!("query result: {:?}", result);
    log::info!("exec result: {:?}", result2);

    log::info!("running another query");

    let result = execute(
        "SELECT * FROM foo WHERE bar = ?".to_owned(),
        vec!["baz".to_owned()],
    )
    .await;
    log::info!("result: {:?}", result);

    Ok(())
}

init_reducer!(Mutation, reducer);
