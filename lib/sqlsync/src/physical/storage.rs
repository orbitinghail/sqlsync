use anyhow::Result;

use super::{
    changeset::Changeset, cursor::Cursor, layer::Layer, sqlite_shm::SqliteShm,
    sqlite_wal::SqliteWal,
};

pub struct Storage {
    layer_id_gen: u64,
    layers: Vec<Layer>,
    wal: SqliteWal,
    shm: SqliteShm,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            layer_id_gen: 0,
            layers: Vec::new(),
            wal: SqliteWal::new(),
            shm: SqliteShm::new(),
        }
    }

    pub fn maybe_checkpoint(&mut self, max_pages: usize) -> Result<()> {
        if self.wal.num_pages() > max_pages {
            self.checkpoint()
        } else {
            Ok(())
        }
    }

    fn checkpoint(&mut self) -> Result<()> {
        // get next layer_id
        let layer_id = self.layer_id_gen;
        self.layer_id_gen += 1;

        // save the current wal into a layer
        let layer = Layer::new(layer_id, self.wal.as_pages());
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
