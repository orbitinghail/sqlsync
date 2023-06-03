use std::{cell::RefCell, rc::Rc};

use log::debug;
use rusqlite::{session::Session, Connection, OpenFlags, Transaction};
use sqlite_vfs::register;

use crate::vfs::{self, PAGESIZE};

pub struct Database {
    db: Connection,
    storage: Rc<RefCell<vfs::Storage>>,
}

impl Database {
    // new
    pub fn new() -> Self {
        let storage = Rc::new(RefCell::new(vfs::Storage::new()));
        let v = vfs::VirtualVfs {
            storage: storage.clone(),
        };

        register("vfs", v).unwrap();

        Self {
            db: Self::connection(),
            storage,
        }
    }

    pub fn connection() -> Connection {
        let db = Connection::open_with_flags_and_vfs(
            "main.db",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            "vfs",
        )
        .unwrap();

        db.pragma_update(None, "page_size", PAGESIZE).unwrap();
        db.pragma_update(None, "cache_size", 0).unwrap();
        db.pragma_update(None, "journal_mode", "wal").unwrap();
        db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        db.pragma_update(None, "synchronous", "off").unwrap();

        db
    }

    pub fn commit(&mut self) {
        debug!("commit");
        self.db
            .pragma(None, "wal_checkpoint", "TRUNCATE", |_| Ok(()))
            .unwrap();
    }

    pub fn rollback(&mut self) {
        self.storage.borrow_mut().rollback()
    }

    // run a closure on db in a txn
    pub fn run<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut Transaction) -> anyhow::Result<()>,
    {
        let mut txn = self.db.transaction()?;
        f(&mut txn)?; // will cause a rollback on failure
        txn.commit()?;
        Ok(())
    }

    // run a closure on db in a txn, rolling back any changes
    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        f(self.db.transaction()?)
        // will drop the tx right away, throwing away any changes
    }

    pub fn session(&self) -> anyhow::Result<Session> {
        let session = Session::new(&self.db)?;
        Ok(session)
    }
}
