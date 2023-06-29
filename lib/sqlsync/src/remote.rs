use std::collections::HashMap;

use rusqlite::Connection;

use crate::{
    db::open_with_vfs,
    journal::{Cursor, Journal, JournalPartial},
    physical::{SparsePages, Storage},
    Mutator,
};

pub struct Remote<M: Mutator> {
    storage: Box<Storage>,
    journals: HashMap<u64, Journal<M::Mutation>>,
    sqlite: Connection,
}

impl<M: Mutator> Remote<M> {
    pub fn new(mutator: M) -> Self {
        let (sqlite, storage) = open_with_vfs().expect("failed to open sqlite db");

        Self {
            storage,
            sqlite,
            journals: HashMap::new(),
        }
    }

    pub fn handle_client_mutations(
        &mut self,
        client_id: u64,
        partial: JournalPartial<M::Mutation>,
    ) -> Cursor {
        // get the journal for this client (or create a new one)
        let journal = self
            .journals
            .entry(client_id)
            .or_insert_with(|| Journal::new());

        journal.sync_receive(partial)
    }

    pub fn handle_client_sync_storage(
        &self,
        client_cursor: Cursor,
    ) -> JournalPartial<'_, SparsePages> {
        // let storage = self.storage.borrow();
        // storage.sync_prepare(client_cursor)
        todo!()
    }
}
