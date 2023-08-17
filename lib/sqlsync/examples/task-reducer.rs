// build task-reducer.wasm using: "cargo build --target wasm32-unknown-unknown --example task-reducer"

use std::panic;

use serde::{Deserialize, Serialize};
use sqlsync_reducer::{execute, init_reducer, query, types::ReducerError};

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,

    AppendTask {
        id: i64,
        description: String,
    },

    RemoveTask {
        id: i64,
    },

    UpdateTask {
        id: i64,
        description: Option<String>,
        completed: Option<bool>,
    },

    MoveTask {
        id: i64,
        after: i64,
    },
}

async fn query_max_sort() -> Result<f64, ReducerError> {
    let response = query!("select max(sort) from tasks").await;
    assert!(response.rows.len() == 1, "expected 1 row");
    Ok(response.rows[0].maybe_get(0)?.unwrap_or(0.0))
}

async fn query_sort_after(id: i64) -> Result<f64, ReducerError> {
    let response = query!(
        "
            select sort, next_sort from (
                select id, sort, lead(sort) over w as next_sort
                from tasks
                window w as (order by sort rows between current row and 1 following)
            ) where id = ?
        ",
        id
    )
    .await;

    if response.rows.len() == 0 {
        query_max_sort().await
    } else {
        let row = &response.rows[0];
        let sort: f64 = row.get(0)?;
        let next_sort: Option<f64> = row.maybe_get(1)?;
        Ok(match next_sort {
            Some(next_sort) => (sort + next_sort) / 2.,
            None => sort + 1.,
        })
    }
}

async fn reducer(mutation: Mutation) -> Result<(), ReducerError> {
    match mutation {
        Mutation::InitSchema => {
            execute!(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id INTEGER PRIMARY KEY,
                    sort DOUBLE UNIQUE NOT NULL,
                    description TEXT NOT NULL,
                    completed BOOLEAN NOT NULL,
                    created_at TEXT NOT NULL
                )"
            )
            .await;
        }

        Mutation::AppendTask { id, description } => {
            log::debug!("appending task({}): {}", id, description);
            let max_sort = query_max_sort().await?;
            execute!(
                "insert into tasks (id, sort, description, completed, created_at)
                    values (?, ?, ?, false, datetime('now'))",
                id,
                max_sort + 1.,
                description
            )
            .await;
        }

        Mutation::RemoveTask { id } => {
            execute!("delete from tasks where id = ?", id).await;
        }

        Mutation::UpdateTask {
            id,
            description,
            completed,
        } => {
            execute!(
                "update tasks set
                    description = IFNULL(?, description),
                    completed = IFNULL(?, completed)
                 where id = :id",
                id,
                description,
                completed
            )
            .await;
        }

        Mutation::MoveTask { id, after } => {
            let new_sort = query_sort_after(after).await?;
            execute!("update tasks set sort = ? where id = ?", new_sort, id);
        }
    }

    Ok(())
}

init_reducer!(Mutation, reducer);
