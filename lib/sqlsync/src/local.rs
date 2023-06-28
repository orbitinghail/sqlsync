use std::{cell::RefCell, rc::Rc};

use rusqlite::{Connection, OpenFlags, Transaction};

use crate::{
    db::{readyonly_query, run_in_tx},
    physical::PAGESIZE,
    vfs::{RcStorage, StorageVfs},
};

pub struct Local {
    storage: RcStorage,
    sqlite: Connection,
}

impl Local {
    pub fn new() -> Self {
        let storage = RcStorage::new();

        // register the vfs globally (BOOOOOO)
        let v = Rc::new(RefCell::new(StorageVfs::new(storage.clone())));
        sqlite_vfs::register("local-vfs", v).expect("failed to register local-vfs with sqlite");

        let sqlite = Connection::open_with_flags_and_vfs(
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

        Self { storage, sqlite }
    }

    pub fn run<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut Transaction) -> anyhow::Result<()>,
    {
        run_in_tx(&mut self.sqlite, f)
    }

    // run a closure on db in a txn, rolling back any changes
    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        readyonly_query(&mut self.sqlite, f)
    }
}
