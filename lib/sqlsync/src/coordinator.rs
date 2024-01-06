use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::convert::From;
use std::fmt::Debug;
use std::io;

use rusqlite::Transaction;

use crate::db::{open_with_vfs, run_in_tx, ConnectionPair};
use crate::error::Result;
use crate::reducer::{Reducer, WasmReducer};
use crate::replication::{
    ReplicationDestination, ReplicationError, ReplicationSource,
};
use crate::timeline::{apply_timeline_range, run_timeline_migration};
use crate::Lsn;
use crate::{
    journal::{Journal, JournalFactory, JournalId},
    lsn::LsnRange,
    storage::Storage,
};

struct ReceiveQueueEntry {
    id: JournalId,
    range: LsnRange,
}

pub struct CoordinatorDocument<J: Journal, R> {
    reducer: R,
    storage: Box<Storage<J>>,
    sqlite: ConnectionPair,
    timeline_factory: J::Factory,
    timelines: HashMap<JournalId, J>,
    timeline_receive_queue: VecDeque<ReceiveQueueEntry>,
}

impl<J: Journal, R> Debug for CoordinatorDocument<J, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CoordinatorDocument")
            .field(&self.storage)
            .field(&("timelines", &self.timelines.values()))
            .finish()
    }
}

impl<J: Journal, R: Reducer> CoordinatorDocument<J, R> {
    pub fn open(
        storage: J,
        timeline_factory: J::Factory,
        reducer: R, // reducer_wasm_bytes: &[u8],
    ) -> Result<Self> {
        let (mut sqlite, mut storage) = open_with_vfs(storage)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite.readwrite)?;
        storage.commit()?;

        Ok(Self {
            reducer,
            storage,
            sqlite,
            timeline_factory,
            timelines: HashMap::new(),
            timeline_receive_queue: VecDeque::new(),
        })
    }

    fn get_or_create_timeline_mut(
        &mut self,
        id: JournalId,
    ) -> io::Result<&mut J> {
        match self.timelines.entry(id) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                Ok(entry.insert(self.timeline_factory.open(id)?))
            }
        }
    }

    pub fn has_pending_work(&self) -> bool {
        !self.timeline_receive_queue.is_empty()
    }

    fn mark_received(&mut self, id: JournalId, lsn: Lsn) {
        match self.timeline_receive_queue.back_mut() {
            // coalesce this update if the queue already ends with an entry for this journal
            Some(entry) if entry.id == id => {
                if !entry.range.contains(lsn) {
                    entry.range = entry.range.append(lsn)
                }
            }
            // otherwise, just push a new entry
            _ => self.timeline_receive_queue.push_back(ReceiveQueueEntry {
                id,
                range: LsnRange::new(lsn, lsn),
            }),
        }
    }

    pub fn mutate_direct<F, E>(&mut self, f: F) -> Result<(), E>
    where
        F: FnOnce(&mut Transaction) -> Result<(), E>,
        E: From<rusqlite::Error> + From<io::Error>,
    {
        run_in_tx(&mut self.sqlite.readwrite, f)?;
        self.storage.commit()?;
        Ok(())
    }

    pub fn step(&mut self) -> Result<()> {
        // check to see if we have anything in the receive queue
        let entry = self.timeline_receive_queue.pop_front();

        if let Some(entry) = entry {
            log::debug!(
                "applying range {} to timeline {}",
                entry.range,
                entry.id
            );

            // get the timeline
            let timeline = self
                .timelines
                .get(&entry.id)
                .expect("timeline missing in timelines but present in the receive queue");

            // apply part of the timeline (per the receive queue entry) to the db
            apply_timeline_range(
                timeline,
                &mut self.sqlite.readwrite,
                &mut self.reducer,
                entry.range,
            )?;

            // commit changes
            self.storage.commit()?;
        }

        Ok(())
    }
}

/// CoordinatorDocument knows how to replicate it's storage journal
impl<J: Journal + ReplicationSource, R: Reducer> ReplicationSource
    for CoordinatorDocument<J, R>
{
    type Reader<'a> = <J as ReplicationSource>::Reader<'a>
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.storage.source_id()
    }

    fn source_range(&self) -> LsnRange {
        self.storage.source_range()
    }

    fn read_lsn<'a>(
        &'a self,
        lsn: crate::Lsn,
    ) -> io::Result<Option<Self::Reader<'a>>> {
        self.storage.read_lsn(lsn)
    }
}

/// CoordinatorDocument knows how to receive timeline journals from elsewhere
impl<J: Journal + ReplicationDestination, R: Reducer> ReplicationDestination
    for CoordinatorDocument<J, R>
{
    fn range(
        &mut self,
        id: JournalId,
    ) -> std::result::Result<LsnRange, ReplicationError> {
        let timeline = self.get_or_create_timeline_mut(id)?;
        ReplicationDestination::range(timeline, id)
    }

    fn write_lsn<Reader>(
        &mut self,
        id: JournalId,
        lsn: crate::Lsn,
        reader: &mut Reader,
    ) -> std::result::Result<(), ReplicationError>
    where
        Reader: io::Read,
    {
        let timeline = self.get_or_create_timeline_mut(id)?;
        timeline.write_lsn(id, lsn, reader)?;
        self.mark_received(id, lsn);
        Ok(())
    }
}
