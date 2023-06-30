use rusqlite::{named_params, Connection};

use std::fmt::Debug;

use crate::{
    db::run_in_tx,
    journal::{Cursor, Journal, JournalPartial},
    Mutator,
};

pub type TimelineId = u64;

const MAX_SYNC: usize = 10;

const TIMELINES_TABLE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS __sqlsync_timelines (
        id INTEGER PRIMARY KEY,
        lsn INTEGER NOT NULL
    )
";

const TIMELINES_READ_CURSOR_SQL: &str = "
    SELECT lsn
    FROM __sqlsync_timelines
    WHERE id = :id
";

const TIMELINES_UPDATE_CURSOR_SQL: &str = "
    INSERT INTO __sqlsync_timelines (id, lsn)
    VALUES (:id, :lsn)
    ON CONFLICT (id) DO UPDATE SET lsn = :lsn
";

pub fn run_timeline_migration(sqlite: &mut Connection) -> anyhow::Result<()> {
    sqlite.execute(TIMELINES_TABLE_SQL, [])?;
    Ok(())
}

pub struct Timeline<M: Mutator> {
    id: TimelineId,
    mutator: M,
    journal: Journal<M::Mutation>,
}

impl<M: Mutator> Debug for Timeline<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Timeline")
            .field(&self.id)
            .field(&self.journal)
            .finish()
    }
}

impl<M: Mutator> Timeline<M> {
    pub fn new(id: TimelineId, mutator: M) -> Self {
        Self {
            id,
            mutator,
            journal: Journal::new(),
        }
    }

    pub fn run(&mut self, sqlite: &mut Connection, mutation: M::Mutation) -> anyhow::Result<()> {
        let out = run_in_tx(sqlite, |tx| self.mutator.apply(tx, &mutation));
        self.journal.append(mutation);
        out
    }

    pub fn sync_prepare(&self, cursor: Cursor) -> JournalPartial<M::Mutation> {
        self.journal.sync_prepare(cursor, MAX_SYNC)
    }

    pub fn rebase(&mut self, sqlite: &mut Connection) -> anyhow::Result<()> {
        let applied_cursor: Cursor = sqlite.query_row(
            TIMELINES_READ_CURSOR_SQL,
            named_params! {":id": self.id},
            |row| row.get(0),
        )?;

        // remove mutations from the journal that have already been applied
        self.journal.remove_up_to(applied_cursor);

        // reapply remaining mutations in the journal
        run_in_tx(sqlite, |tx| {
            self.journal
                .iter()
                .map(|mutation| self.mutator.apply(tx, &mutation))
                .collect::<anyhow::Result<_>>()
        })?;

        Ok(())
    }
}

pub struct RemoteTimeline<M: Mutator> {
    pub id: TimelineId,
    mutator: M,
    journal: Journal<M::Mutation>,
}

impl<M: Mutator> Debug for RemoteTimeline<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RemoteTimeline")
            .field(&self.id)
            .field(&self.journal)
            .finish()
    }
}

impl<M: Mutator> RemoteTimeline<M> {
    pub fn new(id: TimelineId, mutator: M) -> Self {
        Self {
            id,
            mutator,
            journal: Journal::new(),
        }
    }

    pub fn sync_receive(&mut self, partial: JournalPartial<M::Mutation>) -> Cursor {
        self.journal.sync_receive(partial)
    }

    pub fn apply_up_to(&mut self, sqlite: &mut Connection, cursor: Cursor) -> anyhow::Result<()> {
        run_in_tx(sqlite, |tx| {
            let start_cursor = tx
                .query_row(
                    TIMELINES_READ_CURSOR_SQL,
                    named_params! {":id": self.id},
                    |row| row.get(0),
                )
                // on success, we want to start at the next position in the log
                .map(|c: Cursor| c.next())
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(Cursor::new(0)),
                    _ => Err(err),
                })?;

            // Collecting into a Result<Vec> from from a Vec<Result> will stop
            // iterating at the first error
            self.journal
                .iter_range(start_cursor, cursor)
                .map(|mutation| self.mutator.apply(tx, &mutation))
                .collect::<anyhow::Result<_>>()?;

            // if we successfully apply all the above mutations update
            // the cursor in the db
            tx.execute(
                TIMELINES_UPDATE_CURSOR_SQL,
                rusqlite::named_params! {
                    ":id": &self.id,
                    ":lsn": &cursor,
                },
            )?;

            Ok(())
        })
    }
}
