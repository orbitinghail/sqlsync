// build: "cargo build --target wasm32-unknown-unknown -p demo-reducer --release"

use serde::Deserialize;
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Deserialize, Debug)]
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
            execute!(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    description TEXT NOT NULL,
                    completed BOOLEAN NOT NULL,
                    created_at TEXT NOT NULL
                )"
            )
            .await;
        }

        Mutation::CreateTask { id, description } => {
            log::debug!("appending task({}): {}", id, description);
            execute!(
                "insert into tasks (id, description, completed, created_at)
                    values (?, ?, false, datetime('now'))",
                id,
                description
            )
            .await;
        }

        Mutation::DeleteTask { id } => {
            execute!("delete from tasks where id = ?", id).await;
        }

        Mutation::ToggleCompleted { id } => {
            execute!(
                "update tasks set completed = not completed where id = ?",
                id
            )
            .await;
        }
    }

    Ok(())
}
