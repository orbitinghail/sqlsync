use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Serialize, Deserialize)]
enum Mutation {
    Set(String, String),
    Delete(String),
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
    let mutation: Mutation = bincode::deserialize(&mutation)?;
    match mutation {
        Mutation::Set(key, value) => {
            execute!(
                "INSERT INTO kv (key, value) VALUES (?, ?)
                ON CONFLICT (key) DO UPDATE SET value = VALUES(value)",
                key,
                value
            )
            .await;
        }
        Mutation::Delete(key) => {
            execute!("DELETE FROM kv WHERE key = ?", key).await;
        }
    }

    Ok(())
}
