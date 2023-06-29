use std::collections::{btree_map::Iter, BTreeMap};

use super::PAGESIZE;

pub type PageIdx = u64;
pub type Page = [u8; PAGESIZE];

#[derive(Default, Debug, Clone)]
pub struct SparsePages {
    pages: BTreeMap<PageIdx, Page>,
}

impl SparsePages {
    pub fn new() -> SparsePages {
        Self {
            pages: BTreeMap::new(),
        }
    }

    pub fn num_pages(&self) -> usize {
        self.pages.len()
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn iter(&self) -> Iter<'_, PageIdx, Page> {
        self.pages.iter()
    }

    // returns the max page index of this sparse pages object
    pub fn max_page_idx(&self) -> Option<PageIdx> {
        self.pages.keys().max().copied()
    }

    pub fn write(&mut self, page_idx: PageIdx, page: Page) {
        self.pages.insert(page_idx, page);
    }

    pub fn read(&self, page_idx: PageIdx) -> Option<&Page> {
        self.pages.get(&page_idx)
    }
}
