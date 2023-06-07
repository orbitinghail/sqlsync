use std::collections::btree_map::Iter;

use super::{
    cursor::Cursor,
    page::{Page, PageIdx, SparsePages},
};

pub struct Changeset {
    cursor: Cursor,
    pages: SparsePages,
}

impl Changeset {
    pub fn iter(&self) -> Iter<'_, PageIdx, Page> {
        self.pages.iter()
    }
}
