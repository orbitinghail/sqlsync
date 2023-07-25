use anyhow::Result;
use rusqlite::{Connection, OpenFlags, Transaction};
use sqlite_vfs::FilePtr;

use crate::{
    journal::Journal,
    physical::{Storage, PAGESIZE},
    vfs::StorageVfs,
};

pub fn open_with_vfs<J: Journal>(journal: J) -> Result<(Connection, Box<Storage<J>>)> {
    let mut storage = Box::new(Storage::new(journal));
    let storage_ptr = FilePtr::new(&mut storage);

    // generate random vfs name
    let vfs_name = format!("local-vfs-{}", rand::random::<u64>());

    // register the vfs globally
    let vfs = StorageVfs::new(storage_ptr);
    sqlite_vfs::register(&vfs_name, vfs).expect("failed to register local-vfs with sqlite");

    let sqlite = Connection::open_with_flags_and_vfs(
        "main.db",
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        &vfs_name,
    )
    .unwrap();

    sqlite.pragma_update(None, "page_size", PAGESIZE).unwrap();
    sqlite.pragma_update(None, "synchronous", "off").unwrap();
    sqlite
        .pragma_update(None, "journal_mode", "memory")
        .unwrap();

    // TODO: benchmark with/without cache
    // sqlite.pragma_update(None, "default_cache_size", 0).unwrap();
    // sqlite.pragma_update(None, "cache_size", 0).unwrap();

    Ok((sqlite, storage))
}

pub fn run_in_tx<F>(sqlite: &mut Connection, f: F) -> Result<()>
where
    F: FnOnce(&mut Transaction) -> Result<()>,
{
    let mut txn = sqlite.transaction()?;
    f(&mut txn)?; // will cause a rollback on failure
    txn.commit()?;
    Ok(())
}

// run a closure on db in a txn, rolling back any changes
pub fn readonly_query<F>(sqlite: &mut Connection, f: F) -> Result<()>
where
    F: FnOnce(Transaction) -> Result<()>,
{
    // TODO: this is a hack to get around rusqlite's lack of support for readonly txns
    //       this can be better enforced by wrapping the tx in something that rejects commit
    f(sqlite.transaction()?)
    // will drop the tx right away, throwing away any changes
}
