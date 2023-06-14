use rusqlite::{Connection, Transaction};

pub fn run_in_tx<F>(sqlite: &mut Connection, f: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut Transaction) -> anyhow::Result<()>,
{
    let mut txn = sqlite.transaction()?;
    f(&mut txn)?; // will cause a rollback on failure
    txn.commit()?;
    Ok(())
}

// run a closure on db in a txn, rolling back any changes
pub fn readyonly_query<F>(sqlite: &mut Connection, f: F) -> anyhow::Result<()>
where
    F: FnOnce(Transaction) -> anyhow::Result<()>,
{
    f(sqlite.transaction()?)
    // will drop the tx right away, throwing away any changes
}
