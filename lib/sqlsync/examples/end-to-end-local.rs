use std::collections::HashMap;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use sqlsync::{
    client::ClientDocument,
    mutate::Mutator,
    named_params,
    positioned_io::{PositionedCursor, PositionedReader},
    server::ServerDocument,
    Deserializable, Document, LsnRange, MemoryJournal, MutableDocument, OptionalExtension,
    QueryableDocument, RequestedLsnRange, Serializable, SteppableDocument, Transaction,
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

fn query_task(tx: &Transaction, id: &i64) -> anyhow::Result<Option<Task>> {
    let mut stmt =
        tx.prepare("select id, sort, description, completed, created_at from tasks where id = ?")?;
    let task: Option<Task> = stmt
        .query_row([id], |row| {
            Ok(Task {
                id: row.get(0)?,
                sort: row.get(1)?,
                description: row.get(2)?,
                completed: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .optional()?;

    Ok(task)
}

fn query_task_exists(tx: &Transaction, id: &i64) -> anyhow::Result<bool> {
    let mut stmt = tx.prepare("select exists(select 1 from tasks where id = ?)")?;
    let exists: bool = stmt.query_row([id], |row| row.get(0))?;
    Ok(exists)
}

fn query_sort_after(tx: &Transaction, id: &i64) -> anyhow::Result<f64> {
    // query for the sort of the task with the given id and the sort of the task immediately following it (or None)
    let mut stmt = tx.prepare(
        "
            select sort, next_sort from (
                select id, sort, lead(sort) over w as next_sort
                from tasks
                window w as (order by sort rows between current row and 1 following)
            ) where id = ?
        ",
    )?;

    let sorts = stmt
        .query_row([id], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, Option<f64>>(1)?))
        })
        .optional()?;

    match sorts {
        Some((sort, Some(next_sort))) => Ok((sort + next_sort) / 2.),
        Some((sort, None)) => Ok(sort + 1.),
        None => Err(anyhow!("task not found")),
    }
}

fn query_max_sort(tx: &Transaction) -> anyhow::Result<f64> {
    let mut stmt = tx.prepare("select max(sort) from tasks")?;
    let max_sort: Option<f64> = stmt.query_row([], |row| row.get(0))?;
    Ok(max_sort.unwrap_or(0.))
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct PartialTask {
    description: Option<String>,
    completed: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,

    AppendTask { id: i64, description: String },
    RemoveTask { id: i64 },
    UpdateTask { id: i64, partial: PartialTask },
    MoveTask { id: i64, after: i64 },
}

impl Serializable for Mutation {
    fn serialize_into<W: std::io::Write>(&self, writer: &mut W) -> anyhow::Result<()> {
        Ok(bincode::serialize_into(writer, &self)?)
    }
}

impl Deserializable for Mutation {
    fn deserialize_from<R: PositionedReader>(reader: R) -> anyhow::Result<Self> {
        Ok(bincode::deserialize_from(PositionedCursor::new(reader))?)
    }
}

#[derive(Clone)]
struct MutatorImpl {}

impl Mutator for MutatorImpl {
    type Mutation = Mutation;

    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()> {
        match mutation {
            Mutation::InitSchema => tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id INTEGER PRIMARY KEY,
                    sort DOUBLE UNIQUE NOT NULL,
                    description TEXT NOT NULL,
                    completed BOOLEAN NOT NULL,
                    created_at TEXT NOT NULL
                )",
            )?,

            Mutation::AppendTask { id, description } => {
                log::debug!("appending task({}): {}", id, description);
                let max_sort = query_max_sort(tx)?;
                tx.execute(
                    "insert into tasks (id, sort, description, completed, created_at)
                    values (:id, :sort, :description, false, datetime('now'))",
                    named_params! { ":id": id, ":sort": max_sort+1., ":description": description },
                )
                .map(|_| ())?
            }

            Mutation::RemoveTask { id } => tx
                .execute(
                    "delete from tasks where id = :id",
                    named_params! { ":id": id },
                )
                .map(|_| ())?,

            Mutation::UpdateTask { id, partial } => {
                let task = query_task(tx, id)?;

                if let Some(task) = task {
                    tx.execute(
                        "update tasks set
                            description = :description,
                            completed = :completed
                        where id = :id",
                        named_params! {
                            ":id": id,
                            ":description": partial.description.as_ref().unwrap_or(&task.description),
                            ":completed": partial.completed.as_ref().unwrap_or(&task.completed),
                        },
                    )
                    .map(|_| ())?
                }
            }

            Mutation::MoveTask { id, after } => {
                if query_task_exists(tx, id)? {
                    let new_sort = query_sort_after(tx, after)?;
                    tx.execute(
                        "update tasks set sort = :sort where id = :id",
                        named_params! { ":id": id, ":sort": new_sort },
                    )
                    .map(|_| ())?
                }
            }
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .env()
        .init()?;

    let doc_id = 1;
    let m = MutatorImpl {};

    let mut local: ClientDocument<MemoryJournal, _> = ClientDocument::open(doc_id, m.clone())?;
    let mut local2: ClientDocument<MemoryJournal, _> = ClientDocument::open(doc_id, m.clone())?;
    let mut remote: ServerDocument<MemoryJournal, _> = ServerDocument::open(doc_id, m.clone())?;

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
            $client.mutate(Mutation::InitSchema)?
        };
        ($client:ident, AppendTask $id:literal, $description:literal) => {
            log::info!(
                "{}: appending task {} {}",
                std::stringify!($client),
                $id,
                $description
            );
            $client.mutate(Mutation::AppendTask {
                id: $id,
                description: $description.into(),
            })?
        };
        ($client:ident, RemoveTask $id:literal) => {
            log::info!("{}: removing task {}", std::stringify!($client), $id);
            $client.mutate(Mutation::RemoveTask { id: $id })?
        };
        ($client:ident, UpdateTask $id:literal, $partial:expr) => {
            log::info!("{}: updating task {}", std::stringify!($client), $id);
            $client.mutate(Mutation::UpdateTask {
                id: $id,
                partial: $partial,
            })?
        };
        ($client:ident, MoveTask $id:literal after $after:literal) => {
            log::info!(
                "{}: moving task {} after {}",
                std::stringify!($client),
                $id,
                $after
            );
            $client.mutate(Mutation::MoveTask {
                id: $id,
                after: $after,
            })?
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

            if let Some(partial) = $from.sync_prepare(req)? {
                let range = $to.sync_receive(partial)?;
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
