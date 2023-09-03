///! This example demonstrates setting up sqlsync in process between two clients
///! and a server. There is no networking in this example so it's easy to follow
///! the sync & rebase logic between the different nodes.
///
use std::{collections::BTreeMap, format, io};

use serde::{Deserialize, Serialize};
use sqlsync::{
    coordinator::CoordinatorDocument, local::LocalDocument, replication::ReplicationProtocol,
    sqlite::Transaction, JournalId, MemoryJournal, MemoryJournalFactory,
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

    let doc_id = JournalId::new128();
    // build task_reducer.wasm using: `cargo build --target wasm32-unknown-unknown --example task-reducer`
    let wasm_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/examples/task_reducer.wasm");

    let mut local = LocalDocument::open(
        MemoryJournal::open(doc_id)?,
        MemoryJournal::open(JournalId::new128())?,
        &wasm_bytes[..],
    )?;
    let mut local2 = LocalDocument::open(
        MemoryJournal::open(doc_id)?,
        MemoryJournal::open(JournalId::new128())?,
        &wasm_bytes[..],
    )?;
    let mut remote = CoordinatorDocument::open(
        MemoryJournal::open(doc_id)?,
        MemoryJournalFactory,
        &wasm_bytes[..],
    )?;

    let mut protocols = BTreeMap::new();
    protocols.insert("local->remote", ReplicationProtocol::new());
    protocols.insert("remote->local", ReplicationProtocol::new());
    protocols.insert("local2->remote", ReplicationProtocol::new());
    protocols.insert("remote->local2", ReplicationProtocol::new());

    macro_rules! protocol {
        ($from:ident -> $to:ident) => {{
            let key = format!("{}->{}", stringify!($from), stringify!($to));
            protocols.get_mut(key.as_str()).unwrap()
        }};
    }

    let mut empty_reader = io::empty();

    macro_rules! debug_state {
        (start $($log_args:tt)+) => {
            log::info!("===============================");
            debug_state!(inner);
            log::info!($($log_args)+);
        };
        (inner) => {
            log::info!("LOCAL1: {:?}", local);
            log::info!("LOCAL2: {:?}", local2);
            log::info!("{:?}", remote);
        };
        (finish) => {
            debug_state!(inner);
            log::info!("===============================");
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
                Ok::<_, anyhow::Error>(())
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
        ($client:ident, AppendTask $id:literal, $description:expr) => {
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

    macro_rules! send {
        ($from:ident -> $to:ident, $msg:expr, $reader:expr) => {
            log::info!(
                "sending: {:?} from {} to {}",
                $msg,
                stringify!($from),
                stringify!($to)
            );

            if let Some(resp) = protocol!($to -> $from).handle(&mut $to, $msg, $reader)? {
                log::info!("received: {:?}", resp);

                if let Some(resp) = protocol!($from -> $to).handle(&mut $from, resp, &mut empty_reader)? {
                    panic!(
                        "unexpected response, send! can only handle one round trip: {:?}",
                        resp
                    );
                }
            }
        };
    }

    macro_rules! connect {
        ($from:ident, $to:ident) => {
            let msg = protocol!($from -> $to).start(&mut $from);
            send!($from -> $to, msg, &mut empty_reader);

            let msg = protocol!($to -> $from).start(&mut $to);
            send!($to -> $from, msg, &mut empty_reader);
        }
    }

    macro_rules! sync {
        ($from:ident -> $to:ident) => {
            debug_state!(start "syncing: {} -> {}", stringify!($from), stringify!($to));

            let mut num_sent = 0;

            while let Some((msg, reader)) = protocol!($from -> $to).sync(&$from)? {
                // we copy here in order to release the mut borrow on protocols
                // this is just for local testing without the network
                let mut reader = &reader.to_owned()[..];
                send!($from -> $to, msg, &mut reader);
                num_sent += 1;
            }

            if num_sent > 0 {
                log::info!("{}: synced {} frames", stringify!($from), num_sent);
            }else {
                log::info!("{}: nothing to sync", stringify!($from));
            }
            debug_state!(finish);
        };
    }

    macro_rules! rebase {
        ($client:ident) => {
            log::info!("rebasing: {}", stringify!($client));
            $client.rebase()?;
        };
    }

    // initialize replication protocols
    connect!(local, remote);
    connect!(local2, remote);

    // start the workload
    mutate!(local, InitSchema);

    // init should be idempotent
    mutate!(local2, InitSchema);
    // and let's say that before anything else happened local2 did some stuff
    mutate!(local2, AppendTask 4, "does this work?");

    sync!(local -> remote);

    step_remote!();

    sync!(remote -> local);
    rebase!(local);
    print_tasks!(local)?;

    sync!(remote -> local2);
    rebase!(local2);
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
    rebase!(local);
    sync!(remote -> local2);
    rebase!(local2);

    print_tasks!(local)?;
    print_tasks!(local2)?;

    log::info!("DONE");

    Ok(())
}
