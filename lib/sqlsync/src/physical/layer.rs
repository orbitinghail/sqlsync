use super::page::SparsePages;

pub type LayerId = u64;

pub struct Layer {
    id: LayerId,
    pages: SparsePages,
}

