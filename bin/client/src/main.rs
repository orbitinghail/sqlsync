use anyhow::anyhow;
use sqlsync::{named_params, Mutator, OptionalExtension, Transaction};

#[derive(Debug)]
struct Task {
    id: i64,
    sort: f64,
    description: String,
    completed: bool,
}

fn query_tasks(tx: Transaction) -> anyhow::Result<Vec<Task>> {
    let mut stmt =
        tx.prepare("select id, sort, description, completed from tasks order by sort")?;
    let rows = stmt.query_map([], |row| {
        Ok(Task {
            id: row.get(0)?,
            sort: row.get(1)?,
            description: row.get(2)?,
            completed: row.get(3)?,
        })
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }

    Ok(tasks)
}

fn query_task(tx: &Transaction, id: &i64) -> anyhow::Result<Option<Task>> {
    let mut stmt = tx.prepare("select id, sort, description, completed from tasks where id = ?")?;
    let task: Option<Task> = stmt
        .query_row([id], |row| {
            Ok(Task {
                id: row.get(0)?,
                sort: row.get(1)?,
                description: row.get(2)?,
                completed: row.get(3)?,
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

#[derive(Clone)]
struct PartialTask {
    description: Option<String>,
    completed: Option<bool>,
}

#[derive(Clone)]
enum Mutation {
    InitSchema,

    AppendTask { id: i64, description: String },
    RemoveTask { id: i64 },
    UpdateTask { id: i64, partial: PartialTask },
    MoveTask { id: i64, after: i64 },
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
                    completed BOOLEAN NOT NULL
                )",
            )?,

            Mutation::AppendTask { id, description } => {
                let max_sort = query_max_sort(tx)?;
                tx.execute(
                    "insert into tasks (id, sort, description, completed)
                    values (:id, :sort, :description, false)",
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

    let local_id = 1;
    let mut local = sqlsync::Local::new(local_id, MutatorImpl {});
    let mut remote = sqlsync::Remote::new(MutatorImpl {});

    macro_rules! debug_state {
        (start $name:literal) => {
            log::info!("===============================");
            debug_state!(finish);
            log::info!($name);
        };
        (finish) => {
            log::info!("{:?}", local);
            log::info!("{:?}", remote);
        };
    };

    macro_rules! print_tasks {
        () => {
            local.query(|conn| {
                let tasks = query_tasks(conn)?;
                log::info!("{} Tasks:", tasks.len());
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
        (InitSchema) => {
            log::info!("initializing schema");
            local.run(Mutation::InitSchema)?
        };
        (AppendTask $id:literal, $description:literal) => {
            log::info!("appending task {} {}", $id, $description);
            local.run(Mutation::AppendTask {
                id: $id,
                description: $description.into(),
            })?
        };
        (RemoveTask $id:literal) => {
            log::info!("removing task {}", $id);
            local.run(Mutation::RemoveTask { id: $id })?
        };
        (UpdateTask $id:literal, $partial:expr) => {
            log::info!("updating task {}", $id);
            local.run(Mutation::UpdateTask {
                id: $id,
                partial: $partial,
            })?
        };
        (MoveTask $id:literal after $after:literal) => {
            log::info!("moving task {} after {}", $id, $after);
            local.run(Mutation::MoveTask {
                id: $id,
                after: $after,
            })?
        };
    }

    macro_rules! sync {
        (client -> server) => {
            debug_state!(start "syncing: client -> server");
            let req = local.sync_timeline_prepare();
            let server_cursor = remote.handle_client_sync_timeline(local_id, req);
            local.sync_timeline_response(server_cursor);
            debug_state!(finish);
        };
        (server -> client) => {
            debug_state!(start "syncing: server -> client");
            let req = local.storage_cursor();
            let resp = remote.handle_client_sync_storage(req);
            resp.and_then(|r| Some(local.sync_storage_receive(r)))
                .transpose()?;
            debug_state!(finish);
        };
    }

    mutate!(InitSchema);

    sync!(client -> server);
    step_remote!();

    mutate!(AppendTask 1, "work on sqlsync");
    mutate!(AppendTask 2, "eat lunch");
    print_tasks!()?;

    sync!(server -> client);
    print_tasks!()?;

    sync!(client -> server);
    print_tasks!()?;

    step_remote!();

    sync!(server -> client);
    print_tasks!()?;

    // now let's switch the order of the tasks and do a full sync

    mutate!(MoveTask 1 after 2);
    print_tasks!()?;

    // full sync
    sync!(client -> server);
    step_remote!();
    sync!(server -> client);

    print_tasks!()?;

    // run some updates, appends, and removes

    mutate!(UpdateTask 1, PartialTask {
        description: Some("work on sqlsync".into()),
        completed: Some(true),
    });

    mutate!(AppendTask 3, "another one");

    mutate!(RemoveTask 2);

    print_tasks!()?;

    sync!(client -> server);
    step_remote!();
    sync!(server -> client);

    print_tasks!()?;

    Ok(())
}
