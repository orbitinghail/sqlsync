use std::fmt::Debug;

use anyhow::Result;
use rand::Rng;
use rusqlite::{Connection, Transaction};

use crate::{
    db::{open_with_vfs, readonly_query, run_in_tx},
    journal::{Journal, JournalId, JournalPartial},
    lsn::{LsnRange, RequestedLsnRange},
    mutate::Mutator,
    physical::Storage,
    timeline::{rebase_timeline, run_timeline_migration},
};

use super::{Document, DocumentId, MutableDocument, QueryableDocument};

pub struct ClientDocument<J: Journal, M: Mutator> {
    mutator: M,
    timeline: J,
    storage: Box<Storage<J>>,
    sqlite: Connection,
}

impl<J: Journal, M: Mutator> Debug for ClientDocument<J, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ClientDocument")
            .field(&("timeline", &self.timeline))
            .field(&self.storage)
            .finish()
    }
}

impl<J: Journal, M: Mutator> Document<J, M> for ClientDocument<J, M> {
    fn open(id: DocumentId, mutator: M) -> Result<Self> {
        let storage_journal = J::open(id)?;
        let (mut sqlite, storage) = open_with_vfs(storage_journal)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite)?;

        // TODO: we need to do Ids next!
        let timeline_id: u8 = rand::thread_rng().gen();
        let timeline = J::open(timeline_id as JournalId)?;

        Ok(Self {
            mutator,
            timeline,
            storage,
            sqlite,
        })
    }

    fn sync_prepare(&self, req: RequestedLsnRange) -> Result<Option<JournalPartial<J::Iter<'_>>>> {
        Ok(self.timeline.sync_prepare(req)?)
    }

    fn sync_receive(&mut self, partial: JournalPartial<J::Iter<'_>>) -> Result<LsnRange> {
        self.storage.revert();
        let out = self.storage.sync_receive(partial)?;
        rebase_timeline(&mut self.timeline, &mut self.sqlite, &self.mutator)?;
        Ok(out)
    }
}

impl<J: Journal, M: Mutator> MutableDocument<M> for ClientDocument<J, M> {
    fn mutate(&mut self, m: <M as Mutator>::Mutation) -> Result<()> {
        run_in_tx(&mut self.sqlite, |tx| self.mutator.apply(tx, &m))?;
        self.timeline.append(m)?;
        Ok(())
    }
}

impl<J: Journal, M: Mutator> QueryableDocument for ClientDocument<J, M> {
    fn query<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(Transaction) -> Result<()>,
    {
        readonly_query(&mut self.sqlite, f)
    }
}
