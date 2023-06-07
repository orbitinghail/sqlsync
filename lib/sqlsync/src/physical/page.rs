use std::collections::{btree_map::Iter, BTreeMap};

use super::PAGESIZE;

pub type PageIdx = u64;
pub type Page = [u8; PAGESIZE];

pub struct SparsePages {
    pages: BTreeMap<PageIdx, Page>,
}

impl SparsePages {
    pub fn iter(&self) -> Iter<'_, PageIdx, Page> {
        self.pages.iter()
    }
}
