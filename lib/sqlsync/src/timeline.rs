use rusqlite::{named_params, Connection};

use crate::{
    db::run_in_tx,
    journal::{Deserializable, Journal},
    lsn::{Lsn, LsnRange},
    mutate::Mutator,
};

const TIMELINES_TABLE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS __sqlsync_timelines (
        id INTEGER PRIMARY KEY,
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

pub fn run_timeline_migration(sqlite: &mut Connection) -> anyhow::Result<()> {
    sqlite.execute(TIMELINES_TABLE_SQL, [])?;
    Ok(())
}

pub fn rebase_timeline<J: Journal, M: Mutator>(
    timeline: &mut J,
    sqlite: &mut Connection,
    mutator: &M,
) -> anyhow::Result<()> {
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
        timeline
            .iter()?
            .map(|entry| {
                let mutation = M::Mutation::deserialize_from(entry)?;
                mutator.apply(tx, &mutation)
            })
            .collect::<anyhow::Result<_>>()
    })?;

    Ok(())
}

pub fn apply_timeline_range<J: Journal, M: Mutator>(
    timeline: &J,
    sqlite: &mut Connection,
    mutator: &M,
    range: LsnRange,
) -> anyhow::Result<()> {
    run_in_tx(sqlite, |tx| {
        // we first need to potentially trim the range if some or all of it has already been applied
        let range: Option<LsnRange> = tx
            .query_row(
                TIMELINES_READ_LSN_SQL,
                named_params! {":id": timeline.id()},
                |row| row.get(0),
            )
            // trim the range to ensure we don't double apply a mutation
            .map(|applied_lsn: u64| range.trim_prefix(applied_lsn))
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(Some(range)),
                _ => Err(err),
            })?;

        log::debug!("applying range: {:?}", range);

        if let Some(range) = range {
            // ok, some or all of the provided range needs to be applied so let's do that

            // Collecting into a Result<Vec> from from a Vec<Result> will stop
            // iterating at the first error
            timeline
                .iter_range(range)?
                .map(|entry| {
                    let mutation = M::Mutation::deserialize_from(entry)?;
                    mutator.apply(tx, &mutation)
                })
                .collect::<anyhow::Result<_>>()?;

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
        }

        Ok(())
    })
}
