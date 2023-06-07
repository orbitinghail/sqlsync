use anyhow::Result;

use super::{
    changeset::Changeset, cursor::Cursor, layer::Layer, sqlite_shm::SqliteShm,
    sqlite_wal::SqliteWal,
};

pub struct Storage {
    layers: Vec<Layer>,
    wal: SqliteWal,
    shm: SqliteShm,
}

impl Storage {
    fn maybe_checkpoint(&mut self) -> Result<()> {
        todo!()
    }

    fn diff(&mut self, cursor: Cursor) -> Result<Changeset> {
        todo!()
    }
}
