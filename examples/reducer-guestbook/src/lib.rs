use serde::Deserialize;
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Deserialize, Debug)]
#[serde(tag = "tag")]
enum Mutation {
    InitSchema,
    AddMessage { id: String, msg: String },
    DeleteMessage { id: String },
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = serde_json::from_slice(&mutation[..])?;

    match mutation {
        Mutation::InitSchema => {
            execute!(
                "CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY,
                    msg TEXT NOT NULL,
                    created_at TEXT NOT NULL
                )"
            )
            .await?;
        }

        Mutation::AddMessage { id, msg } => {
            log::info!("appending message({}): {}", id, msg);
            execute!(
                "insert into messages (id, msg, created_at) values (?, ?, datetime('now'))",
                id,
                msg
            )
            .await?;
        }

        Mutation::DeleteMessage { id } => {
            execute!("delete from messages where id = ?", id).await?;
        }
    }

    Ok(())
}
