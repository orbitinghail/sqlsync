use super::page::{PageIdx, SparsePages};

pub type LayerId = u64;

pub struct Layer {
    id: LayerId,
    pages: SparsePages,
}

impl Layer {
    pub fn new(id: LayerId, pages: SparsePages) -> Self {
        Self { id, pages }
    }

    pub fn max_page_idx(&self) -> PageIdx {
        self.pages.iter().map(|(idx, _)| *idx).max().unwrap_or(0)
    }
}
