// build: "cargo build --target wasm32-unknown-unknown -p counter-reducer"
use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "tag")]
enum Mutation {
    InitSchema,
    Incr { value: i32 },
    Decr { value: i32 },
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = serde_json::from_slice(&mutation[..])?;

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

        Mutation::Incr { value } => {
            execute!(
                "INSERT INTO counter (id, value) VALUES (0, 0)
                ON CONFLICT (id) DO UPDATE SET value = value + ?",
                value
            )
            .await;
        }

        Mutation::Decr { value } => {
            execute!(
                "INSERT INTO counter (id, value) VALUES (0, 0)
                ON CONFLICT (id) DO UPDATE SET value = value - ?",
                value
            )
            .await;
        }
    }

    Ok(())
}
