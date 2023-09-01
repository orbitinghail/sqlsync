use std::io;

use rusqlite::{named_params, Connection, Transaction};
use thiserror::Error;

use crate::{
    journal::{Cursor, Journal},
    lsn::{Lsn, LsnRange},
    positioned_io::PositionedReader,
    reducer::{Reducer, ReducerError},
    JournalError, ScanError,
};

const TIMELINES_TABLE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS __sqlsync_timelines (
        id BLOB PRIMARY KEY,
        lsn INTEGER NOT NULL
    )
";

const TIMELINES_READ_LSN_SQL: &str = "
    SELECT lsn
    FROM __sqlsync_timelines
    WHERE id = :id
";

const TIMELINES_UPDATE_LSN_SQL: &str = "
    INSERT INTO __sqlsync_timelines (id, lsn)
    VALUES (:id, :lsn)
    ON CONFLICT (id) DO UPDATE SET lsn = :lsn
";

#[derive(Error, Debug)]
pub enum TimelineError {
    #[error("io error: {0}")]
    IoError(#[from] io::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    JournalError(#[from] JournalError),

    #[error(transparent)]
    ScanError(#[from] ScanError),

    #[error(transparent)]
    ReducerError(#[from] ReducerError),
}

type Result<T> = std::result::Result<T, TimelineError>;

fn run_in_tx<F>(sqlite: &mut Connection, f: F) -> Result<()>
where
    F: FnOnce(&mut Transaction) -> Result<()>,
{
    let mut txn = sqlite.transaction()?;
    f(&mut txn)?; // will cause a rollback on failure
    txn.commit()?;
    Ok(())
}

pub fn run_timeline_migration(sqlite: &mut Connection) -> Result<()> {
    sqlite.execute(TIMELINES_TABLE_SQL, [])?;
    Ok(())
}

pub fn apply_mutation<J: Journal>(
    timeline: &mut J,
    sqlite: &mut Connection,
    reducer: &mut Reducer,
    mutation: &[u8],
) -> Result<()> {
    run_in_tx(sqlite, |tx| Ok(reducer.apply(tx, &mutation)?))?;
    timeline.append(mutation)?;
    Ok(())
}

pub fn rebase_timeline<J: Journal>(
    timeline: &mut J,
    sqlite: &mut Connection,
    reducer: &mut Reducer,
) -> Result<()> {
    let applied_lsn: Option<Lsn> = sqlite
        .query_row(
            TIMELINES_READ_LSN_SQL,
            named_params! {":id": timeline.id()},
            |row| row.get(0),
        )
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            err => Err(err),
        })?;

    log::debug!("rebase timeline ({:?}) to lsn {:?}", timeline, applied_lsn);

    // remove mutations from the journal that have already been applied
    if let Some(applied_lsn) = applied_lsn {
        timeline.drop_prefix(applied_lsn)?;
    }

    // reapply remaining mutations in the journal
    run_in_tx(sqlite, |tx| {
        let mut cursor = timeline.scan();
        while cursor.advance()? {
            let mutation = cursor.read_all()?;
            reducer.apply(tx, &mutation)?;
        }
        Ok(())
    })?;

    Ok(())
}

pub fn apply_timeline_range<J: Journal>(
    timeline: &J,
    sqlite: &mut Connection,
    reducer: &mut Reducer,
    range: LsnRange,
) -> Result<()> {
    // nothing to apply, optimistically return
    if range.is_empty() {
        return Ok(());
    }

    run_in_tx(sqlite, |tx| {
        // we first need to potentially trim the range if some or all of it has already been applied
        let range = tx
            .query_row(
                TIMELINES_READ_LSN_SQL,
                named_params! {":id": timeline.id()},
                |row| row.get(0),
            )
            // trim the range to ensure we don't double apply a mutation
            .map(|applied_lsn: u64| range.trim_prefix(applied_lsn))
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(range),
                _ => Err(err),
            })?;

        if range.is_empty() {
            // nothing to apply, optimistically return
            Ok(())
        } else {
            log::debug!("applying range: {:?}", range);

            // ok, some or all of the provided range needs to be applied so let's do that
            let mut cursor = timeline.scan_range(range);
            while cursor.advance()? {
                let mutation = cursor.read_all()?;
                reducer.apply(tx, &mutation)?;
            }

            log::debug!(
                "updating timeline {} to lsn {:?}",
                timeline.id(),
                range.last()
            );

            // if we successfully apply all the above mutations update
            // the cursor in the db
            tx.execute(
                TIMELINES_UPDATE_LSN_SQL,
                rusqlite::named_params! {
                    ":id": timeline.id(),
                    ":lsn": &range.last(),
                },
            )?;
            Ok(())
        }
    })

    // TODO: once the above tx commits we can GC applied entries in the timeline
}
