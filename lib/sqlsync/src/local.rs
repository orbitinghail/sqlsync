use std::{cell::RefCell, rc::Rc};

use rusqlite::{Connection, OpenFlags, Transaction};

use crate::{
    physical::{StorageReplica, PAGESIZE},
    vfs::VirtualVfs,
};

pub struct Local {
    storage: Rc<RefCell<StorageReplica>>,
    sqlite: Connection,
}

impl Local {
    pub fn new() -> Self {
        let storage = Rc::new(RefCell::new(StorageReplica::new()));

        // register the vfs globally (BOOOOOO)
        let v = VirtualVfs::new(storage.clone());
        sqlite_vfs::register("local-vfs", v).expect("failed to register local-vfs with sqlite");

        let sqlite = Connection::open_with_flags_and_vfs(
            "main.db",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            "local-vfs",
        )
        .unwrap();

        sqlite.pragma_update(None, "page_size", PAGESIZE).unwrap();
        sqlite.pragma_update(None, "cache_size", 0).unwrap();
        sqlite.pragma_update(None, "journal_mode", "wal").unwrap();
        sqlite.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        sqlite.pragma_update(None, "synchronous", "off").unwrap();

        Self { storage, sqlite }
    }

    pub fn run<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut Transaction) -> anyhow::Result<()>,
    {
        let mut txn = self.sqlite.transaction()?;
        f(&mut txn)?; // will cause a rollback on failure
        txn.commit()?;
        Ok(())
    }

    // run a closure on db in a txn, rolling back any changes
    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        f(self.sqlite.transaction()?)
        // will drop the tx right away, throwing away any changes
    }
}
