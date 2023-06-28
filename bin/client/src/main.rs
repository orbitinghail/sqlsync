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

    let mut local = sqlsync::Local::new();

    let print_tasks = |local: &mut sqlsync::Local| {
        local.query(|conn| {
            let tasks = query_tasks(conn)?;
            println!("got {} tasks", tasks.len());
            for task in tasks {
                println!("{:?}", task);
            }
            Ok(())
        })
    };

    println!("initializing schema");
    local
        .run(|tx| {
            let mutator = MutatorImpl {};
            mutator.apply(tx, &Mutation::InitSchema)
        })
        .unwrap();

    println!("appending a task");
    local
        .run(|tx| {
            let mutator = MutatorImpl {};
            mutator.apply(
                tx,
                &Mutation::AppendTask {
                    id: 1,
                    description: "work on sqlsync".into(),
                },
            )
        })
        .unwrap();

    println!("printing tasks");
    print_tasks(&mut local)?;

    // recorder.apply(Mutation::InitSchema)?;
    // recorder.rebase(recorder.seq())?;

    // recorder.apply(Mutation::AppendTask {
    //     id: 1,
    //     description: "work on sqlsync".into(),
    // })?;
    // recorder.apply(Mutation::AppendTask {
    //     id: 2,
    //     description: "eat lunch".into(),
    // })?;

    // print_tasks(&mut recorder)?;

    // recorder.rebase(recorder.seq())?;

    // print_tasks(&mut recorder)?;

    // recorder.apply(Mutation::MoveTask { id: 1, after: 2 })?;

    // print_tasks(&mut recorder)?;

    // recorder.apply(Mutation::UpdateTask {
    //     id: 2,
    //     partial: PartialTask {
    //         description: None,
    //         completed: Some(true),
    //     },
    // })?;

    // print_tasks(&mut recorder)?;

    // recorder.apply(Mutation::RemoveTask { id: 2 })?;

    // recorder.apply(Mutation::AppendTask {
    //     id: 3,
    //     description: "eat lunch".into(),
    // })?;

    // recorder.apply(Mutation::AppendTask {
    //     id: 4,
    //     description: "another one".into(),
    // })?;

    // print_tasks(&mut recorder)?;

    // recorder.apply(Mutation::MoveTask { id: 4, after: 1 })?;

    // print_tasks(&mut recorder)?;

    Ok(())
}
