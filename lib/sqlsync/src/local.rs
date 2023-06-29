use rusqlite::{Connection, Transaction};

use std::fmt::Debug;

use crate::{
    db::{open_with_vfs, readyonly_query},
    journal::{Cursor, JournalPartial},
    logical::{run_timeline_migration, Timeline, TimelineId},
    physical::{SparsePages, Storage},
    Mutator,
};

pub struct Local<M: Mutator> {
    storage: Box<Storage>,
    timeline: Timeline<M>,
    sqlite: Connection,
    server_timeline_cursor: Option<Cursor>,
}

impl<M: Mutator> Debug for Local<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Local")
            .field(&self.timeline)
            .field(&self.storage)
            .field(&("server_timeline_cursor", &self.server_timeline_cursor))
            .finish()
    }
}

impl<M: Mutator> Local<M> {
    pub fn new(timeline_id: TimelineId, mutator: M) -> Self {
        let (mut sqlite, storage) = open_with_vfs().expect("failed to open sqlite db");

        run_timeline_migration(&mut sqlite).expect("failed to initialize timelines table");

        let timeline = Timeline::new(timeline_id, mutator);

        Self {
            storage,
            timeline,
            sqlite,
            server_timeline_cursor: None,
        }
    }

    pub fn run(&mut self, m: M::Mutation) -> anyhow::Result<()> {
        self.timeline.run(&mut self.sqlite, m)
    }

    // run a closure on db in a txn, rolling back any changes
    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        readyonly_query(&mut self.sqlite, f)
    }

    pub fn sync_timeline_prepare(&mut self) -> JournalPartial<'_, M::Mutation> {
        let cursor = self
            .server_timeline_cursor
            .map(|c| c.next())
            .unwrap_or(Cursor::new(0));
        self.timeline.sync_prepare(cursor)
    }

    pub fn sync_timeline_response(&mut self, server_cursor: Cursor) {
        self.server_timeline_cursor = Some(server_cursor);
    }

    pub fn storage_cursor(&mut self) -> Option<Cursor> {
        self.storage.cursor()
    }

    pub fn sync_storage_receive(
        &mut self,
        partial: JournalPartial<SparsePages>,
    ) -> anyhow::Result<()> {
        if !partial.is_empty() {
            self.storage.revert();
            self.storage.sync_receive(partial);
            self.timeline.rebase(&mut self.sqlite)
        } else {
            Ok(())
        }
    }
}
