use anyhow::Result;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap};
use std::fmt::Debug;

use rusqlite::Connection;

use crate::db::open_with_vfs;
use crate::timeline::{apply_timeline_range, run_timeline_migration};
use crate::unixtime::unix_timestamp_milliseconds;
use crate::{
    journal::{Journal, JournalId},
    lsn::LsnRange,
    mutate::Mutator,
    physical::Storage,
};

use super::{Document, SteppableDocument};

#[derive(Debug, PartialEq, Eq)]
struct ReceiveQueueEntry {
    id: JournalId,
    range: LsnRange,
    timestamp: i64,
}

// ProcessQueueEntries are naturally ordered from latest to earliest
impl PartialOrd for ReceiveQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReceiveQueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.timestamp.cmp(&other.timestamp).reverse() {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        self.id.cmp(&other.id)
    }
}

pub struct ServerDocument<J: Journal, M: Mutator> {
    mutator: M,
    storage: Box<Storage<J>>,
    sqlite: Connection,
    timelines: HashMap<JournalId, J>,
    timeline_receive_queue: BinaryHeap<ReceiveQueueEntry>,
}

impl<J: Journal, M: Mutator> ServerDocument<J, M> {
    fn get_or_create_timeline_mut(&mut self, id: JournalId) -> Result<&mut J> {
        match self.timelines.entry(id) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => Ok(entry.insert(J::open(id)?)),
        }
    }
}

impl<J: Journal, M: Mutator> Debug for ServerDocument<J, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ServerDocument")
            .field(&self.storage)
            .field(&("timelines", &self.timelines.values()))
            .finish()
    }
}

impl<J: Journal, M: Mutator> Document<J, M> for ServerDocument<J, M> {
    fn open(id: super::DocumentId, mutator: M) -> Result<Self> {
        let storage_journal = J::open(id)?;
        let (mut sqlite, storage) = open_with_vfs(storage_journal)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite)?;

        Ok(Self {
            mutator,
            storage,
            sqlite,
            timelines: HashMap::new(),
            timeline_receive_queue: BinaryHeap::new(),
        })
    }

    fn sync_prepare(
        &self,
        req: crate::lsn::RequestedLsnRange,
    ) -> Result<Option<crate::journal::JournalPartial<<J as Journal>::Iter<'_>>>> {
        Ok(self.storage.sync_prepare(req)?)
    }

    fn sync_receive(
        &mut self,
        partial: crate::journal::JournalPartial<<J as Journal>::Iter<'_>>,
    ) -> Result<LsnRange> {
        let id = partial.id;
        let timeline = self.get_or_create_timeline_mut(id)?;
        let range = timeline.sync_receive(partial)?;

        // TODO: this can sometimes queue more work than needed - specifically
        // if we have already received the partial, the associated range will
        // have also already been applied. This is fine since applying the same
        // range multiple times is idempotent, but may be worth fixing at some
        // point
        self.timeline_receive_queue.push(ReceiveQueueEntry {
            id,
            range,
            timestamp: unix_timestamp_milliseconds(),
        });
        Ok(range)
    }
}

impl<J: Journal, M: Mutator> SteppableDocument for ServerDocument<J, M> {
    fn has_pending_work(&self) -> bool {
        !self.timeline_receive_queue.is_empty()
    }

    fn step(&mut self) -> anyhow::Result<()> {
        // check to see if we have anything in the receive queue
        let entry = self.timeline_receive_queue.pop();

        if let Some(entry) = entry {
            log::debug!("applying {:?} on timeline {}", entry.range, entry.id);

            // get the timeline
            let timeline = self.timelines.get(&entry.id).ok_or_else(|| {
                anyhow::anyhow!("timeline missing in timelines but present in the receive queue")
            })?;

            // apply part of the timeline (per the receive queue entry) to the db
            apply_timeline_range(timeline, &mut self.sqlite, &self.mutator, entry.range)?;

            // commit changes
            self.storage.commit()?;

            // TODO: announce that we have new data to all clients
        }

        Ok(())
    }
}
