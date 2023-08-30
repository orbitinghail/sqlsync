use std::{fmt::Debug, io, result::Result};

use rusqlite::{Connection, Transaction};

use crate::{
    db::{open_with_vfs, readonly_query, run_in_tx},
    journal::{Journal, JournalId},
    lsn::LsnRange,
    reducer::Reducer,
    replication::{ReplicationDestination, ReplicationError, ReplicationSource},
    storage::Storage,
    timeline::{rebase_timeline, run_timeline_migration},
};

pub struct LocalDocument<J> {
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

impl<J: Journal + ReplicationSource> LocalDocument<J> {
    pub fn open(storage: J, timeline: J, reducer_wasm_bytes: &[u8]) -> anyhow::Result<Self> {
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

    pub fn doc_id(&self) -> JournalId {
        self.storage.source_id()
    }

    pub fn mutate(&mut self, m: &[u8]) -> anyhow::Result<()> {
        run_in_tx(&mut self.sqlite, |tx| self.reducer.apply(tx, &m))?;
        self.timeline.append(m)?;
        Ok(())
    }

    pub fn query<F, O>(&mut self, f: F) -> anyhow::Result<O>
    where
        F: FnOnce(Transaction) -> anyhow::Result<O>,
    {
        readonly_query(&mut self.sqlite, f)
    }

    pub fn rebase(&mut self) -> anyhow::Result<()> {
        if self.storage.has_committed_pages() {
            self.storage.revert();
            rebase_timeline(&mut self.timeline, &mut self.sqlite, &mut self.reducer)?;
        }
        Ok(())
    }
}

/// LocalDocument knows how to send it's timeline journal elsewhere
impl<J: ReplicationSource> ReplicationSource for LocalDocument<J> {
    type Reader<'a> = <J as ReplicationSource>::Reader<'a>
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.timeline.source_id()
    }

    fn read_lsn<'a>(&'a self, lsn: crate::Lsn) -> io::Result<Option<Self::Reader<'a>>> {
        self.timeline.read_lsn(lsn)
    }
}

/// LocalDocument knows how to receive a storage journal from elsewhere
impl<J: ReplicationDestination> ReplicationDestination for LocalDocument<J> {
    fn range(&mut self, id: JournalId) -> Result<Option<LsnRange>, ReplicationError> {
        self.storage.range(id)
    }

    fn write_lsn<R>(
        &mut self,
        id: JournalId,
        lsn: crate::Lsn,
        reader: &mut R,
    ) -> Result<(), ReplicationError>
    where
        R: io::Read,
    {
        self.storage.write_lsn(id, lsn, reader)
    }
}
