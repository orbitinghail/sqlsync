// build: "cargo build --target wasm32-unknown-unknown -p demo-reducer --release"

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "tag")]
enum Mutation {
    InitSchema,

    CreateTask { id: String, description: String },

    DeleteTask { id: String },

    ToggleCompleted { id: String },
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = serde_json::from_slice(&mutation[..])?;

    match mutation {
        Mutation::InitSchema => {
            futures::join!(execute!(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    description TEXT NOT NULL,
                    completed BOOLEAN NOT NULL,
                    created_at TEXT NOT NULL
                )"
            ));
        }

        Mutation::CreateTask { id, description } => {
            log::debug!("appending task({}): {}", id, description);
            futures::join!(execute!(
                "insert into tasks (id, description, completed, created_at)
                    values (?, ?, false, datetime('now'))",
                id,
                description
            ));
        }

        Mutation::DeleteTask { id } => {
            futures::join!(execute!("delete from tasks where id = ?", id));
        }

        Mutation::ToggleCompleted { id } => {
            futures::join!(execute!(
                "update tasks set completed = not completed where id = ?",
                id
            ));
        }
    }

    Ok(())
}
