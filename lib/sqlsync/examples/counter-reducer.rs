// build: "cargo build --target wasm32-unknown-unknown --example counter-reducer"

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,
    Incr,
    Decr,
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = bincode::deserialize(&mutation)?;
    match mutation {
        Mutation::InitSchema => {
            futures::join!(
                execute!(
                    "CREATE TABLE IF NOT EXISTS counter (
                    id INTEGER PRIMARY KEY,
                    value INTEGER
                )"
                ),
                execute!("INSERT OR IGNORE INTO counter (id, value) VALUES (0, 0)")
            );
        }
        Mutation::Incr => {
            execute!(
                "INSERT INTO counter (id, value) VALUES (0, 0)
                ON CONFLICT (id) DO UPDATE SET value = value + 1"
            )
            .await;
        }
        Mutation::Decr => {
            execute!(
                "INSERT INTO counter (id, value) VALUES (0, 0)
                ON CONFLICT (id) DO UPDATE SET value = value - 1"
            )
            .await;
        }
    }

    Ok(())
}
