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
    pub fn maybe_checkpoint(&mut self, max_pages: usize) -> Result<()> {
        if self.wal.num_pages() > max_pages {
            self.checkpoint()
        } else {
            Ok(())
        }
    }

    fn checkpoint(&mut self) -> Result<()> {
        // save the current wal into a layer
        let layer = self.wal.as_layer();
        self.layers.push(layer);

        // reset the wal
        self.wal.reset();

        // find the max page id across all layers
        let max_page_id = self
            .layers
            .iter()
            .map(|layer| layer.max_page_idx())
            .max()
            .unwrap_or(0);

        // reset the shm
        self.shm.reset(max_page_id as usize, &self.wal);
        Ok(())
    }

    pub fn diff(&mut self, cursor: Cursor) -> Result<Changeset> {
        todo!()
    }
}
