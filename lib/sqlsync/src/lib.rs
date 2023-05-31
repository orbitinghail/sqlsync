use std::{cell::RefCell, rc::Rc};

use log::debug;
use rusqlite::{Connection, OpenFlags, Transaction};
use sqlite_vfs::register;

mod pagevfs;

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

        debug!("db opened, setting pragmas");
        db.pragma_update(None, "page_size", PAGESIZE).unwrap();
        db.pragma_update(None, "cache_size", 0).unwrap();
        db.pragma_update(None, "locking_mode", "exclusive").unwrap();
        db.pragma_update(None, "journal_mode", "memory").unwrap();
        db.pragma_update(None, "synchronous", "off").unwrap();
        debug!("...done");

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
    pub fn run<F, T>(&mut self, f: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&mut Transaction) -> rusqlite::Result<T>,
    {
        let mut txn = self.db.transaction()?;
        let res = f(&mut txn);
        match res {
            Ok(_) => txn.commit(),
            Err(_) => txn.rollback(),
        }?;

        res
    }
}

// pub struct Recorder<'conn> {
//     session: Session<'conn>,
// }

// impl Recorder<'_> {
//     pub fn new(db: &Database) -> Recorder<'_> {
//         let mut session = Session::new(&db.db).unwrap();
//         session.attach(None).unwrap(); // None = record all changes
//         Recorder { session }
//     }
// }
