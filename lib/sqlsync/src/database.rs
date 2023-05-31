use std::{cell::RefCell, rc::Rc};

use rusqlite::{Connection, OpenFlags, Statement, Transaction};
use sqlite_vfs::register;

use crate::pagevfs;

const PAGESIZE: usize = 4096;

pub struct Database {
    pub(crate) db: Connection,
    storage: Rc<RefCell<pagevfs::Storage<PAGESIZE>>>,
}

impl Database {
    // new
    pub fn new() -> Self {
        let storage = Rc::new(RefCell::new(pagevfs::Storage::<PAGESIZE>::new()));
        let v = pagevfs::StorageVfs::<PAGESIZE> {
            storage: storage.clone(),
        };

        register("pagevfs", v).unwrap();

        Self {
            db: Self::connection(),
            storage,
        }
    }

    pub fn connection() -> Connection {
        let db = Connection::open_with_flags_and_vfs(
            "main.db",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
            "pagevfs",
        )
        .unwrap();

        db.pragma_update(None, "page_size", PAGESIZE).unwrap();
        db.pragma_update(None, "cache_size", 0).unwrap();
        db.pragma_update(None, "locking_mode", "exclusive").unwrap();
        db.pragma_update(None, "journal_mode", "memory").unwrap();
        db.pragma_update(None, "synchronous", "off").unwrap();

        db
    }

    pub fn branch(&mut self) {
        self.storage.borrow_mut().branch()
    }

    pub fn commit(&mut self) {
        self.storage.borrow_mut().commit()
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
}
