use rusqlite::{Connection, Transaction};

use crate::{
    db::{open_with_vfs, readyonly_query},
    journal::JournalPartial,
    logical::Timeline,
    physical::{SparsePages, Storage},
    Mutator,
};

pub struct Local<M: Mutator> {
    storage: Box<Storage>,
    timeline: Timeline<M>,
    sqlite: Connection,
}

impl<M: Mutator> Local<M> {
    pub fn new(mutator: M) -> Self {
        // TODO: get client_id from somewhere
        let client_id = 0;

        let (mut sqlite, storage) = open_with_vfs().expect("failed to open sqlite db");

        let timeline = Timeline::new(client_id, mutator);
        timeline
            .migrate_db(&mut sqlite)
            .expect("failed to run timeline sql migrations");

        Self {
            storage,
            timeline,
            sqlite,
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

    pub fn XXX_DEBUG_commit(&mut self) {
        self.storage.commit()
    }

    pub fn XXX_DEBUG_revert(&mut self) {
        self.storage.revert()
    }

    pub fn rebase(&mut self) -> anyhow::Result<()> {
        let req = self.timeline.sync_request()?;
        let resp: JournalPartial<SparsePages> = todo!("network.send(req)");
        if resp.is_empty() {
            return Ok(());
        }
        self.storage.revert();
        self.storage.sync_receive(resp);

        self.timeline.rebase(&mut self.sqlite)
    }
}
