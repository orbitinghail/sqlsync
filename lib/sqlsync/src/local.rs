use rusqlite::{Connection, Transaction};

use std::fmt::Debug;

use crate::{
    db::{open_with_vfs, readyonly_query},
    journal::{Journal, JournalId, JournalPartial, MemoryJournal},
    logical::{run_timeline_migration, Timeline},
    lsn::{LsnRange, RequestedLsnRange},
    physical::Storage,
    unixtime::UnixTime,
    Mutator,
};

const MAX_TIMELINE_SYNC: usize = 10;

pub struct Local<M: Mutator> {
    storage: Box<Storage<MemoryJournal>>,
    timeline: Timeline<M, MemoryJournal>,
    sqlite: Connection,
    server_timeline_range: Option<LsnRange>,
}

impl<M: Mutator> Debug for Local<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Local")
            .field(&self.timeline)
            .field(&self.storage)
            .field(&("server_timeline_range", &self.server_timeline_range))
            .finish()
    }
}

impl<M: Mutator> Local<M> {
    pub fn new(journal_id: JournalId, mutator: M, unixtime: impl UnixTime) -> Self {
        // TODO: storage id should be initialized from the server or our cache
        // for now, we always set it to 0 on the client and the server
        let journal = MemoryJournal::empty(0);
        let (mut sqlite, storage) =
            open_with_vfs(unixtime, journal).expect("failed to open sqlite db");

        run_timeline_migration(&mut sqlite).expect("failed to initialize timelines table");

        // TODO: journal_id can be loaded from cache or randomly initialized
        let journal = MemoryJournal::empty(journal_id);
        let timeline = Timeline::new(mutator, journal);

        Self {
            storage,
            timeline,
            sqlite,
            server_timeline_range: None,
        }
    }

    pub fn id(&self) -> JournalId {
        self.timeline.id()
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

    pub fn sync_timeline_prepare(
        &mut self,
    ) -> anyhow::Result<Option<JournalPartial<<MemoryJournal as Journal>::Iter<'_>>>> {
        let req = match self.server_timeline_range {
            Some(range) => RequestedLsnRange::new(range.last() + 1, MAX_TIMELINE_SYNC),
            None => RequestedLsnRange::new(0, MAX_TIMELINE_SYNC),
        };
        self.timeline.sync_prepare(req)
    }

    pub fn sync_timeline_response(&mut self, server_range: LsnRange) {
        self.server_timeline_range = Some(server_range);
    }

    pub fn sync_storage_request(&mut self) -> RequestedLsnRange {
        self.storage.sync_request()
    }

    pub fn sync_storage_receive(
        &mut self,
        partial: JournalPartial<<MemoryJournal as Journal>::Iter<'_>>,
    ) -> anyhow::Result<()> {
        self.storage.revert();
        self.storage.sync_receive(partial)?;
        self.timeline.rebase(&mut self.sqlite)
    }
}
