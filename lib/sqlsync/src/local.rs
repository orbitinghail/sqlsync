use std::{fmt::Debug, io};

use anyhow::Result;
use rusqlite::{Connection, Transaction};

use crate::{
    db::{open_with_vfs, readonly_query, run_in_tx},
    journal::{Cursor, Journal, JournalId, JournalPartial, SyncResult, Syncable},
    lsn::{LsnRange, RequestedLsnRange},
    reducer::Reducer,
    storage::Storage,
    timeline::{rebase_timeline, run_timeline_migration},
};

pub struct LocalDocument<J: Journal> {
    reducer: Reducer,
    timeline: J,
    storage: Box<Storage<J>>,
    sqlite: Connection,
}

impl<J: Journal> Debug for LocalDocument<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalDocument")
            .field(&("timeline", &self.timeline))
            .field(&self.storage)
            .finish()
    }
}

impl<J: Journal> LocalDocument<J> {
    pub fn open(storage: J, timeline: J, reducer_wasm_bytes: &[u8]) -> Result<Self> {
        let (mut sqlite, storage) = open_with_vfs(storage)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite)?;

        Ok(Self {
            reducer: Reducer::new(reducer_wasm_bytes)?,
            timeline,
            storage,
            sqlite,
        })
    }

    pub fn mutate(&mut self, m: &[u8]) -> Result<()> {
        run_in_tx(&mut self.sqlite, |tx| self.reducer.apply(tx, &m))?;
        self.timeline.append(m)?;
        Ok(())
    }

    pub fn query<F, O>(&mut self, f: F) -> Result<O>
    where
        F: FnOnce(Transaction) -> Result<O>,
    {
        readonly_query(&mut self.sqlite, f)
    }
}

impl<J: Journal> Syncable for LocalDocument<J> {
    type Cursor<'a> = <J as Syncable>::Cursor<'a> where Self: 'a;

    fn source_id(&self) -> JournalId {
        self.timeline.id()
    }

    fn sync_prepare<'a>(
        &'a mut self,
        req: RequestedLsnRange,
    ) -> SyncResult<Option<JournalPartial<Self::Cursor<'a>>>> {
        self.timeline.sync_prepare(req)
    }

    fn sync_request(&mut self, id: JournalId) -> SyncResult<RequestedLsnRange> {
        self.storage.sync_request(id)
    }

    fn sync_receive<C>(&mut self, partial: JournalPartial<C>) -> SyncResult<LsnRange>
    where
        C: Cursor + io::Read,
    {
        self.storage.revert();
        let out = self.storage.sync_receive(partial)?;
        rebase_timeline(&mut self.timeline, &mut self.sqlite, &mut self.reducer)?;
        Ok(out)
    }
}
