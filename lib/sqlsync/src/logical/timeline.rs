use rusqlite::Connection;

use crate::{
    db::run_in_tx,
    journal::{Cursor, Journal, JournalPartial},
    Mutator,
};

const MAX_SYNC: usize = 10;

const TIMELINES_TABLE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS __sqlsync_timelines (
        id INTEGER PRIMARY KEY,
        lsn INTEGER NOT NULL
    )
";

pub struct Timeline<M: Mutator> {
    id: u64,
    mutator: M,
    journal: Journal<M::Mutation>,
}

impl<M: Mutator> Timeline<M> {
    pub fn new(id: u64, mutator: M) -> Self {
        Self {
            id,
            mutator,
            journal: Journal::new(),
        }
    }

    pub fn migrate_db(&self, sqlite: &mut Connection) -> anyhow::Result<()> {
        run_in_tx(sqlite, |tx| {
            tx.execute(TIMELINES_TABLE_SQL, [])?;
            Ok(())
        })
    }

    pub fn run(&mut self, sqlite: &mut Connection, mutation: M::Mutation) -> anyhow::Result<()> {
        let out = run_in_tx(sqlite, |tx| self.mutator.apply(tx, &mutation));
        self.journal.append(mutation);
        out
    }

    pub fn sync_request(&self) -> anyhow::Result<Cursor> {
        self.journal.end()
    }

    pub fn sync_prepare(&self, cursor: Cursor) -> JournalPartial<M::Mutation> {
        self.journal.sync_prepare(cursor, MAX_SYNC)
    }

    pub fn sync_receive(&mut self, partial: JournalPartial<M::Mutation>) -> Cursor {
        self.journal.sync_receive(partial)
    }

    pub fn rebase(&self, sqlite: &mut Connection) -> anyhow::Result<()> {
        todo!(
            "
            * read seq from timelines table
            * remove mutations <= seq
            * reapply remaining mutations
            "
        )
    }
}
