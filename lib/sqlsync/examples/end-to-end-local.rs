///! This example demonstrates setting up sqlsync in process between two clients
///! and a server. There is no networking in this example so it's easy to follow
///! the sync & rebase logic between the different nodes.
///
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlsync::{
    coordinator::CoordinatorDocument, local::LocalDocument, sqlite::Transaction, Journal,
    JournalId, LsnRange, MemoryJournal, RequestedLsnRange, Syncable,
};

#[derive(Debug)]
#[allow(dead_code)]
struct Task {
    id: i64,
    sort: f64,
    description: String,
    completed: bool,
    created_at: String,
}

fn query_tasks(tx: Transaction) -> anyhow::Result<Vec<Task>> {
    let mut stmt =
        tx.prepare("select id, sort, description, completed, created_at from tasks order by sort")?;
    let rows = stmt.query_map([], |row| {
        Ok(Task {
            id: row.get(0)?,
            sort: row.get(1)?,
            description: row.get(2)?,
            completed: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }

    Ok(tasks)
}

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

fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .env()
        .init()?;

    let doc_id = JournalId::new();
    // build task_reducer.wasm using: `cargo build --target wasm32-unknown-unknown --example task-reducer`
    let wasm_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/examples/task_reducer.wasm");

    let mut local = LocalDocument::open(
        MemoryJournal::open(doc_id)?,
        MemoryJournal::open(JournalId::new())?,
        &wasm_bytes[..],
    )?;
    let mut local2 = LocalDocument::open(
        MemoryJournal::open(doc_id)?,
        MemoryJournal::open(JournalId::new())?,
        &wasm_bytes[..],
    )?;
    let mut remote = CoordinatorDocument::open(MemoryJournal::open(doc_id)?, &wasm_bytes[..])?;

    // temp hack to track the lsn ranges we get back from sync_receive
    // key is a concatenation of the sync direction, like: `local->remote`
    let mut hack_lsn_range_cache: HashMap<String, LsnRange> = HashMap::new();

    macro_rules! debug_state {
        (start $($log_args:tt)+) => {
            log::info!("===============================");
            debug_state!(finish);
            log::info!($($log_args)+);
        };
        (finish) => {
            log::info!("LOCAL1: {:?}", local);
            log::info!("LOCAL2: {:?}", local2);
            log::info!("{:?}", remote);
        };
    }

    macro_rules! print_tasks {
        ($client:ident) => {
            $client.query(|conn| {
                let tasks = query_tasks(conn)?;
                log::info!("{} has {} tasks:", std::stringify!($client), tasks.len());
                for task in tasks {
                    log::info!("  {:?}", task);
                }
                Ok(())
            })
        };
    }

    macro_rules! step_remote {
        () => {
            debug_state!(start "Stepping remote");
            remote.step()?;
            debug_state!(finish);
        };
    }

    macro_rules! mutate {
        ($client:ident, InitSchema) => {
            log::info!("{}: initializing schema", std::stringify!($client));
            mutate!($client, Mutation::InitSchema)?
        };
        ($client:ident, AppendTask $id:literal, $description:literal) => {
            log::info!(
                "{}: appending task {} {}",
                std::stringify!($client),
                $id,
                $description
            );
            mutate!(
                $client,
                Mutation::AppendTask {
                    id: $id,
                    description: $description.into(),
                }
            )?
        };
        ($client:ident, RemoveTask $id:literal) => {
            log::info!("{}: removing task {}", std::stringify!($client), $id);
            mutate!($client, Mutation::RemoveTask { id: $id })?
        };
        ($client:ident, UpdateTask $id:literal, $description:expr, $completed:expr) => {
            log::info!("{}: updating task {}", std::stringify!($client), $id);
            mutate!(
                $client,
                Mutation::UpdateTask {
                    id: $id,
                    description: $description,
                    completed: $completed,
                }
            )?
        };
        ($client:ident, MoveTask $id:literal after $after:literal) => {
            log::info!(
                "{}: moving task {} after {}",
                std::stringify!($client),
                $id,
                $after
            );
            mutate!(
                $client,
                Mutation::MoveTask {
                    id: $id,
                    after: $after,
                }
            )?
        };
        ($client:ident, $mutation:expr) => {
            $client.mutate(&bincode::serialize(&$mutation)?)
        };
    }

    macro_rules! sync {
        ($from:ident -> $to:ident) => {
            // compute request (TODO: replace once we have a LinkManager)
            let key = format!("{}->{}", stringify!($from), stringify!($to));
            let req = match hack_lsn_range_cache.get(&key) {
                Some(range) => RequestedLsnRange::new(range.last()+1, 10),
                None => RequestedLsnRange::new(0, 10),
            };

            debug_state!(start "syncing: {} -> {} ({:?})", stringify!($from), stringify!($to), req);

            let partial = $from.sync_prepare(req)?;
            if let Some(partial) = partial {
                // let partial = partial.map(|e| Ok(PositionedCursor::new(e)));
                let range = $to.sync_receive(partial.into_read_partial())?;
                hack_lsn_range_cache.insert(key, range);
            } else {
                log::info!("{}: nothing to sync", stringify!($from));
            }
            debug_state!(finish);
        };
    }

    mutate!(local, InitSchema);

    // init should be idempotent
    mutate!(local2, InitSchema);
    // and let's say that before anything else happened local2 did some stuff
    mutate!(local2, AppendTask 4, "does this work?");

    sync!(local -> remote);

    step_remote!();

    sync!(remote -> local);
    print_tasks!(local)?;

    sync!(remote -> local2);
    print_tasks!(local2)?;

    // at this point, remote has incorporated changes from local, but not local2
    // let's continue to do work and see if everything converges

    mutate!(local, AppendTask 1, "work on sqlsync");
    mutate!(local2, AppendTask 2, "eat lunch");
    mutate!(local, AppendTask 3, "go on a walk");

    print_tasks!(local)?;
    print_tasks!(local2)?;

    sync!(local -> remote);
    sync!(local2 -> remote);

    // need to step twice to apply changes from both clients
    // should be applied as local then local2
    step_remote!();
    step_remote!();

    // sync down changes
    sync!(remote -> local);
    sync!(remote -> local2);

    print_tasks!(local)?;
    print_tasks!(local2)?;

    log::info!("DONE");

    Ok(())
}
