use rusqlite::{session::Session, Connection, Transaction};

pub struct Database {
    pub(crate) db: Connection,
}

impl Database {
    // new
    pub fn new() -> Self {
        let db = Connection::open_in_memory().unwrap();
        Self { db }
    }

    // run a closure on db in a txn
    fn run<F, T>(&mut self, f: F) -> rusqlite::Result<T>
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

pub struct Recorder<'conn> {
    session: Session<'conn>,
}

impl Recorder<'_> {
    pub fn new(db: &Database) -> Recorder<'_> {
        let mut session = Session::new(&db.db).unwrap();
        session.attach(None).unwrap(); // None = record all changes
        Recorder { session }
    }
}
