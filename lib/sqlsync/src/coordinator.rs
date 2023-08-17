use anyhow::Result;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap};
use std::fmt::Debug;
use std::io;

use rusqlite::Connection;

use crate::db::open_with_vfs;
use crate::journal::{Cursor, JournalPartial, SyncResult, Syncable};
use crate::reducer::Reducer;
use crate::timeline::{apply_timeline_range, run_timeline_migration};
use crate::unixtime::unix_timestamp_milliseconds;
use crate::RequestedLsnRange;
use crate::{
    journal::{Journal, JournalId},
    lsn::LsnRange,
    storage::Storage,
};

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

pub struct CoordinatorDocument<J: Journal> {
    reducer: Reducer,
    storage: Box<Storage<J>>,
    sqlite: Connection,
    timelines: HashMap<JournalId, J>,
    timeline_receive_queue: BinaryHeap<ReceiveQueueEntry>,
}

impl<J: Journal> Debug for CoordinatorDocument<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CoordinatorDocument")
            .field(&self.storage)
            .field(&("timelines", &self.timelines.values()))
            .finish()
    }
}

impl<J: Journal> CoordinatorDocument<J> {
    pub fn open(storage: J, reducer_wasm_bytes: &[u8]) -> Result<Self> {
        let (mut sqlite, storage) = open_with_vfs(storage)?;

        // TODO: this feels awkward here
        run_timeline_migration(&mut sqlite)?;

        Ok(Self {
            reducer: Reducer::new(reducer_wasm_bytes)?,
            storage,
            sqlite,
            timelines: HashMap::new(),
            timeline_receive_queue: BinaryHeap::new(),
        })
    }

    fn get_or_create_timeline_mut(&mut self, id: JournalId) -> Result<&mut J> {
        match self.timelines.entry(id) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => Ok(entry.insert(J::open(id)?)),
        }
    }

    pub fn has_pending_work(&self) -> bool {
        !self.timeline_receive_queue.is_empty()
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        // check to see if we have anything in the receive queue
        let entry = self.timeline_receive_queue.pop();

        if let Some(entry) = entry {
            log::debug!("applying {:?} on timeline {}", entry.range, entry.id);

            // get the timeline
            let timeline = self.timelines.get(&entry.id).ok_or_else(|| {
                anyhow::anyhow!("timeline missing in timelines but present in the receive queue")
            })?;

            // apply part of the timeline (per the receive queue entry) to the db
            apply_timeline_range(timeline, &mut self.sqlite, &mut self.reducer, entry.range)?;

            // commit changes
            self.storage.commit()?;

            // TODO: announce that we have new data to all clients
        }

        Ok(())
    }
}

impl<J: Journal> Syncable for CoordinatorDocument<J> {
    type Cursor<'a> = <J as Syncable>::Cursor<'a> where Self: 'a;

    fn source_id(&self) -> JournalId {
        self.storage.source_id()
    }

    fn sync_prepare<'a>(
        &'a mut self,
        req: RequestedLsnRange,
    ) -> SyncResult<Option<JournalPartial<Self::Cursor<'a>>>> {
        self.storage.sync_prepare(req)
    }

    fn sync_request(&mut self, id: JournalId) -> SyncResult<RequestedLsnRange> {
        let timeline = self.get_or_create_timeline_mut(id)?;
        timeline.sync_request(id)
    }

    fn sync_receive<C>(&mut self, partial: JournalPartial<C>) -> SyncResult<LsnRange>
    where
        C: Cursor + io::Read,
    {
        let id = partial.id();
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
