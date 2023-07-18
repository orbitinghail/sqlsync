use std::{
    collections::{BinaryHeap, HashMap},
    fmt::Debug,
};

use rusqlite::Connection;

use crate::{
    db::open_with_vfs,
    journal::{Journal, JournalId, JournalPartial, MemoryJournal},
    logical::{run_timeline_migration, RemoteTimeline},
    lsn::{LsnRange, RequestedLsnRange},
    physical::Storage,
    unixtime::UnixTime,
    Mutator,
};

#[derive(Debug, PartialEq, Eq)]
struct ReceiveQueueEntry {
    timestamp: i64,
    id: JournalId,
    range: LsnRange,
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

pub struct Remote<M: Mutator, U: UnixTime> {
    mutator: M,
    unixtime: U,
    storage: Box<Storage<MemoryJournal>>,
    timelines: HashMap<JournalId, RemoteTimeline<M, MemoryJournal>>,
    receive_queue: BinaryHeap<ReceiveQueueEntry>,
    sqlite: Connection,
}

impl<M: Mutator, U: UnixTime> Debug for Remote<M, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Remote")
            .field(&self.storage)
            .field(&("timelines", &self.timelines.values()))
            .finish()
    }
}

impl<M: Mutator, U: UnixTime> Remote<M, U> {
    pub fn new(mutator: M, unixtime: U) -> Self {
        // TODO: Remote storage journal needs an Id
        let journal = MemoryJournal::empty(0);
        let (mut sqlite, storage) =
            open_with_vfs(unixtime.clone(), journal).expect("failed to open sqlite db");
        run_timeline_migration(&mut sqlite).expect("failed to initialize timelines table");

        Self {
            mutator,
            unixtime,
            storage,
            sqlite,
            timelines: HashMap::new(),
            receive_queue: BinaryHeap::new(),
        }
    }

    fn get_or_create_timeline_mut(
        &mut self,
        id: JournalId,
    ) -> &mut RemoteTimeline<M, MemoryJournal> {
        self.timelines
            .entry(id)
            .or_insert_with(|| RemoteTimeline::new(self.mutator.clone(), MemoryJournal::empty(id)))
    }

    pub fn handle_client_sync_timeline(
        &mut self,
        journal_id: JournalId,
        partial: JournalPartial<<MemoryJournal as Journal>::Iter<'_>>,
    ) -> anyhow::Result<LsnRange> {
        // get the timeline for this client (or create a new one)
        let timeline = self.get_or_create_timeline_mut(journal_id);

        // store the partial into the journal and get the new range
        let range = timeline.sync_receive(partial)?;

        // add the client to the receive queue
        self.receive_queue.push(ReceiveQueueEntry {
            timestamp: self.unixtime.unix_timestamp_milliseconds(),
            id: journal_id,
            range,
        });

        Ok(range)
    }

    pub fn handle_client_sync_storage(
        &self,
        req: RequestedLsnRange,
    ) -> anyhow::Result<Option<JournalPartial<<MemoryJournal as Journal>::Iter<'_>>>> {
        self.storage.sync_prepare(req)
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        let entry = self.receive_queue.pop();
        if let Some(entry) = entry {
            log::debug!("applying {:?} on timeline {}", entry.range, entry.id);

            // apply the timeline to the db
            let timeline = self
                .timelines
                .get_mut(&entry.id)
                .expect("timeline missing in timelines but present in the receive queue");
            timeline.apply_range(&mut self.sqlite, entry.range)?;

            // commit changes
            self.storage.commit()?;

            // TODO: announce that we have new data to all clients
        }

        Ok(())
    }
}
