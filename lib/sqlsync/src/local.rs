use std::{cell::RefCell, rc::Rc};

use rusqlite::{Connection, OpenFlags, Transaction};

use crate::{
    db::{readyonly_query, run_in_tx},
    logical::Timeline,
    physical::PAGESIZE,
    vfs::{RcStorage, StorageVfs},
    Mutator,
};

pub struct Local<M: Mutator> {
    storage: RcStorage,
    timeline: Timeline<M>,
    sqlite: Connection,
}

impl<M: Mutator> Local<M> {
    pub fn new(mutator: M) -> Self {
        // TODO: get client_id from somewhere
        let client_id = 0;

        let storage = RcStorage::new();

        // register the vfs globally (BOOOOOO)
        let v = Rc::new(RefCell::new(StorageVfs::new(storage.clone())));
        sqlite_vfs::register("local-vfs", v).expect("failed to register local-vfs with sqlite");

        let mut sqlite = Connection::open_with_flags_and_vfs(
            "main.db",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            "local-vfs",
        )
        .unwrap();

        sqlite.pragma_update(None, "page_size", PAGESIZE).unwrap();
        sqlite.pragma_update(None, "cache_size", 0).unwrap();
        sqlite
            .pragma_update(None, "journal_mode", "memory")
            .unwrap();
        sqlite.pragma_update(None, "synchronous", "off").unwrap();
        sqlite
            .pragma_update(None, "locking_mode", "exclusive")
            .unwrap();

        let timeline = Timeline::new(client_id, mutator);
        timeline
            .migrate_db(&mut sqlite)
            .expect("failed to run timeline sql migrations");

        Self {
            storage,
            timeline,
            sqlite,
        }
    }

    pub fn run(&mut self, m: M::Mutation) -> anyhow::Result<()> {
        self.timeline.run(&mut self.sqlite, m)
    }

    // run a closure on db in a txn, rolling back any changes
    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        readyonly_query(&mut self.sqlite, f)
    }
}
