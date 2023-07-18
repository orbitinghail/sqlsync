use rusqlite::{named_params, Connection};

use std::fmt::Debug;

use crate::{
    db::run_in_tx,
    journal::{Deserializable, Journal, JournalId, JournalPartial},
    lsn::{Lsn, LsnRange, RequestedLsnRange},
    Mutator,
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

pub struct Timeline<M: Mutator, J: Journal> {
    mutator: M,
    journal: J,
}

impl<M: Mutator, J: Journal> Debug for Timeline<M, J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Timeline")
            .field(&self.id())
            .field(&self.journal)
            .finish()
    }
}

impl<M: Mutator, J: Journal> Timeline<M, J> {
    pub fn new(mutator: M, journal: J) -> Self {
        Self { mutator, journal }
    }

    pub fn id(&self) -> JournalId {
        self.journal.id()
    }

    pub fn run(&mut self, sqlite: &mut Connection, mutation: M::Mutation) -> anyhow::Result<()> {
        let out = run_in_tx(sqlite, |tx| self.mutator.apply(tx, &mutation));
        self.journal.append(mutation)?;
        out
    }

    pub fn sync_prepare(
        &self,
        req: RequestedLsnRange,
    ) -> anyhow::Result<Option<JournalPartial<J::Iter<'_>>>> {
        Ok(self.journal.sync_prepare(req)?)
    }

    pub fn rebase(&mut self, sqlite: &mut Connection) -> anyhow::Result<()> {
        let applied_lsn: Option<Lsn> = sqlite
            .query_row(
                TIMELINES_READ_LSN_SQL,
                named_params! {":id": self.journal.id()},
                |row| row.get(0),
            )
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                err => Err(err),
            })?;

        log::debug!(
            "rebase timeline ({:?}) to lsn {:?}",
            self.journal,
            applied_lsn
        );

        // remove mutations from the journal that have already been applied
        if let Some(applied_lsn) = applied_lsn {
            self.journal.drop_prefix(applied_lsn)?;
        }

        // reapply remaining mutations in the journal
        run_in_tx(sqlite, |tx| {
            self.journal
                .iter()?
                .map(|entry| {
                    let mutation = M::Mutation::deserialize_from(entry)?;
                    self.mutator.apply(tx, &mutation)
                })
                .collect::<anyhow::Result<_>>()
        })?;

        Ok(())
    }
}

pub struct RemoteTimeline<M: Mutator, J: Journal> {
    mutator: M,
    journal: J,
}

impl<M: Mutator, J: Journal> Debug for RemoteTimeline<M, J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RemoteTimeline")
            .field(&self.id())
            .field(&self.journal)
            .finish()
    }
}

impl<M: Mutator, J: Journal> RemoteTimeline<M, J> {
    pub fn new(mutator: M, journal: J) -> Self {
        Self { mutator, journal }
    }

    pub fn id(&self) -> JournalId {
        self.journal.id()
    }

    pub fn sync_receive(
        &mut self,
        partial: JournalPartial<J::Iter<'_>>,
    ) -> anyhow::Result<LsnRange> {
        Ok(self.journal.sync_receive(partial)?)
    }

    pub fn apply_range(&mut self, sqlite: &mut Connection, range: LsnRange) -> anyhow::Result<()> {
        run_in_tx(sqlite, |tx| {
            // we first need to potentially trim the range if some or all of it has already been applied
            let range: Option<LsnRange> = tx
                .query_row(
                    TIMELINES_READ_LSN_SQL,
                    named_params! {":id": self.id()},
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
                self.journal
                    .iter_range(range)?
                    .map(|entry| {
                        let mutation = M::Mutation::deserialize_from(entry)?;
                        self.mutator.apply(tx, &mutation)
                    })
                    .collect::<anyhow::Result<_>>()?;

                log::debug!("updating timeline {} to lsn {:?}", self.id(), range.last());

                // if we successfully apply all the above mutations update
                // the cursor in the db
                tx.execute(
                    TIMELINES_UPDATE_LSN_SQL,
                    rusqlite::named_params! {
                        ":id": self.id(),
                        ":lsn": &range.last(),
                    },
                )?;
            }

            Ok(())
        })
    }
}
