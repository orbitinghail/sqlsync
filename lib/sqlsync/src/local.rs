use std::{fmt::Debug, io};

use rusqlite::Connection;

use crate::{
    db::{open_with_vfs, ConnectionPair},
    error::Result,
    journal::{Journal, JournalId},
    lsn::LsnRange,
    reducer::WasmReducer,
    replication::{ReplicationDestination, ReplicationError, ReplicationSource},
    storage::{Storage, StorageChange},
    timeline::{apply_mutation, rebase_timeline, run_timeline_migration},
    Lsn,
};

pub trait Signal {
    fn emit(&mut self);
}

pub struct NoopSignal;
impl Signal for NoopSignal {
    fn emit(&mut self) {}
}

pub struct LocalDocument<J, S> {
    reducer: WasmReducer,
    timeline: J,
    storage: Box<Storage<J>>,
    sqlite: ConnectionPair,

    // signals
    storage_changed: S,
    timeline_changed: S,
    rebase_available: S,
}

impl<J: Journal, S> Debug for LocalDocument<J, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalDocument")
            .field(&("timeline", &self.timeline))
            .field(&self.storage)
            .finish()
    }
}

impl<J, S> LocalDocument<J, S>
where
    J: Journal + ReplicationSource,
    S: Signal,
{
    pub fn open(
        storage: J,
        timeline: J,
        reducer: WasmReducer,
        storage_changed: S,
        timeline_changed: S,
        rebase_available: S,
    ) -> Result<Self> {
        let (mut sqlite, storage) = open_with_vfs(storage)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite.readwrite)?;

        Ok(Self {
            reducer,
            timeline,
            storage,
            sqlite,
            storage_changed,
            timeline_changed,
            rebase_available,
        })
    }

    fn signal_storage_change(&mut self) {
        if self.storage.has_changes() {
            self.storage_changed.emit()
        }
    }

    pub fn doc_id(&self) -> JournalId {
        self.storage.source_id()
    }

    pub fn query<F, O, E>(&self, f: F) -> std::result::Result<O, E>
    where
        F: FnOnce(&Connection) -> std::result::Result<O, E>,
        E: std::convert::From<rusqlite::Error>,
    {
        f(&self.sqlite.readonly)
    }

    #[inline]
    pub fn sqlite_readonly(&self) -> &Connection {
        &self.sqlite.readonly
    }

    pub fn mutate(&mut self, m: &[u8]) -> Result<()> {
        apply_mutation(
            &mut self.timeline,
            &mut self.sqlite.readwrite,
            &mut self.reducer,
            m,
        )?;
        self.timeline_changed.emit();
        self.signal_storage_change();
        Ok(())
    }

    pub fn rebase(&mut self) -> Result<()> {
        if self.storage.has_committed_pages() && self.storage.has_invisible_pages() {
            self.storage.reset()?;
            rebase_timeline(
                &mut self.timeline,
                &mut self.sqlite.readwrite,
                &mut self.reducer,
            )?;
            self.signal_storage_change();
        }
        Ok(())
    }

    pub fn storage_changes(&mut self) -> Result<StorageChange> {
        Ok(self.storage.changes()?)
    }

    pub fn storage_lsn(&mut self) -> Option<Lsn> {
        self.storage.last_committed_lsn()
    }
}

/// LocalDocument knows how to send it's timeline journal elsewhere
impl<J: ReplicationSource, S> ReplicationSource for LocalDocument<J, S> {
    type Reader<'a> = <J as ReplicationSource>::Reader<'a>
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.timeline.source_id()
    }

    fn source_range(&self) -> LsnRange {
        self.timeline.source_range()
    }

    fn read_lsn(&self, lsn: crate::Lsn) -> io::Result<Option<Self::Reader<'_>>> {
        self.timeline.read_lsn(lsn)
    }
}

/// LocalDocument knows how to receive a storage journal from elsewhere
impl<J: ReplicationDestination, S: Signal> ReplicationDestination for LocalDocument<J, S> {
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
        let out = self.storage.write_lsn(id, lsn, reader);
        self.rebase_available.emit();
        out
    }
}
