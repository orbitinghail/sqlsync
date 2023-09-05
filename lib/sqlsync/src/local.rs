use std::{fmt::Debug, io};

use rusqlite::{Connection, Transaction};

use crate::{
    db::{open_with_vfs, readonly_query},
    error::Result,
    journal::{Journal, JournalId},
    lsn::LsnRange,
    reducer::Reducer,
    replication::{ReplicationDestination, ReplicationError, ReplicationSource},
    storage::Storage,
    timeline::{apply_mutation, rebase_timeline, run_timeline_migration},
    Lsn,
};

pub struct LocalDocument<J> {
    reducer: Reducer,
    timeline: J,
    storage: Box<Storage<J>>,
    sqlite: Connection,

    // TODO: build a better subscription system
    on_storage_change: Option<Box<dyn Fn()>>,
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
    pub fn open(storage: J, timeline: J, reducer_wasm_bytes: &[u8]) -> Result<Self> {
        let (mut sqlite, storage) = open_with_vfs(storage)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite)?;

        Ok(Self {
            reducer: Reducer::new(reducer_wasm_bytes)?,
            timeline,
            storage,
            sqlite,
            on_storage_change: None,
        })
    }

    pub fn subscribe(&mut self, f: impl Fn() + 'static) {
        // panic if already set
        if self.on_storage_change.is_some() {
            panic!("LocalDocument can't have more than one subscriber");
        }
        self.on_storage_change = Some(Box::new(f));
    }

    pub fn unsubscribe(&mut self) {
        self.on_storage_change = None;
    }

    fn notify_subscription(&self) {
        if let Some(f) = &self.on_storage_change {
            f();
        }
    }

    pub fn doc_id(&self) -> JournalId {
        self.storage.source_id()
    }

    pub fn query<F, O, E>(&mut self, f: F) -> std::result::Result<O, E>
    where
        F: FnOnce(Transaction) -> std::result::Result<O, E>,
        E: std::convert::From<rusqlite::Error>,
    {
        readonly_query(&mut self.sqlite, f)
    }

    pub fn mutate(&mut self, m: &[u8]) -> Result<()> {
        apply_mutation(&mut self.timeline, &mut self.sqlite, &mut self.reducer, m)?;
        self.notify_subscription();
        Ok(())
    }

    pub fn rebase(&mut self) -> Result<()> {
        if self.storage.has_committed_pages() && self.storage.has_invisible_pages() {
            self.storage.revert();
            rebase_timeline(&mut self.timeline, &mut self.sqlite, &mut self.reducer)?;
            self.notify_subscription();
        }
        Ok(())
    }

    pub fn storage_lsn(&mut self) -> Option<Lsn> {
        self.storage.last_committed_lsn()
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

    fn source_range(&self) -> LsnRange {
        self.timeline.source_range()
    }

    fn read_lsn<'a>(&'a self, lsn: crate::Lsn) -> io::Result<Option<Self::Reader<'a>>> {
        self.timeline.read_lsn(lsn)
    }
}

/// LocalDocument knows how to receive a storage journal from elsewhere
impl<J: ReplicationDestination> ReplicationDestination for LocalDocument<J> {
    fn range(&mut self, id: JournalId) -> std::result::Result<LsnRange, ReplicationError> {
        self.storage.range(id)
    }

    fn write_lsn<R>(
        &mut self,
        id: JournalId,
        lsn: crate::Lsn,
        reader: &mut R,
    ) -> std::result::Result<(), ReplicationError>
    where
        R: io::Read,
    {
        self.storage.write_lsn(id, lsn, reader)
    }
}
