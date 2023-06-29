use std::{
    collections::{BinaryHeap, HashMap},
    fmt::Debug,
    time::Instant,
};

use rusqlite::Connection;

use crate::{
    db::open_with_vfs,
    journal::{Cursor, JournalPartial},
    logical::{run_timeline_migration, RemoteTimeline},
    physical::{SparsePages, Storage},
    Mutator,
};

type TimelineId = u64;

#[derive(Debug, PartialEq, Eq, Ord)]
struct ReceiveQueueEntry {
    timestamp: Instant,
    timeline_id: TimelineId,
    cursor: Cursor,
}

// ProcessQueueEntries are naturally ordered from latest to earliest
impl PartialOrd for ReceiveQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self.timestamp.cmp(&other.timestamp).reverse() {
            core::cmp::Ordering::Equal => {}
            ord => return Some(ord),
        }
        self.timeline_id.partial_cmp(&other.timeline_id)
    }
}

pub struct Remote<M: Mutator> {
    mutator: M,
    storage: Box<Storage>,
    timelines: HashMap<TimelineId, RemoteTimeline<M>>,
    receive_queue: BinaryHeap<ReceiveQueueEntry>,
    sqlite: Connection,
}

impl<M: Mutator> Debug for Remote<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Remote")
            .field(&self.storage)
            .field(&("timelines", &self.timelines.values()))
            .finish()
    }
}

impl<M: Mutator> Remote<M> {
    pub fn new(mutator: M) -> Self {
        let (mut sqlite, storage) = open_with_vfs().expect("failed to open sqlite db");
        run_timeline_migration(&mut sqlite).expect("failed to initialize timelines table");

        Self {
            mutator,
            storage,
            sqlite,
            timelines: HashMap::new(),
            receive_queue: BinaryHeap::new(),
        }
    }

    fn get_or_create_timeline_mut(&mut self, timeline_id: TimelineId) -> &mut RemoteTimeline<M> {
        self.timelines
            .entry(timeline_id)
            .or_insert_with(|| RemoteTimeline::new(timeline_id, self.mutator.clone()))
    }

    pub fn handle_client_sync_timeline(
        &mut self,
        timeline_id: TimelineId,
        partial: JournalPartial<M::Mutation>,
    ) -> Cursor {
        // get the timeline for this client (or create a new one)
        let timeline = self.get_or_create_timeline_mut(timeline_id);

        // store the partial into the journal and get the new end cursor
        let cursor = timeline.sync_receive(partial);

        // add the client to the receive queue
        self.receive_queue.push(ReceiveQueueEntry {
            timestamp: Instant::now(),
            timeline_id,
            cursor,
        });

        cursor
    }

    pub fn handle_client_sync_storage(
        &self,
        client_cursor: Option<Cursor>,
    ) -> Option<JournalPartial<'_, SparsePages>> {
        let cursor = client_cursor.map(|c| c.next()).unwrap_or(Cursor::new(0));
        if let Some(storage_cursor) = self.storage.cursor() {
            if cursor <= storage_cursor {
                return Some(self.storage.sync_prepare(cursor));
            }
        }
        None
    }

    pub fn step(&mut self) -> anyhow::Result<()> {
        let entry = self.receive_queue.pop();
        if let Some(entry) = entry {
            // apply the timeline to the db
            let timeline = self
                .timelines
                .get_mut(&entry.timeline_id)
                .expect("timeline missing in timelines but present in the receive queue");
            timeline.apply_up_to(&mut self.sqlite, entry.cursor)?;

            // commit changes
            self.storage.commit();

            // TODO: announce that we have new data to all clients
        }

        Ok(())
    }
}
