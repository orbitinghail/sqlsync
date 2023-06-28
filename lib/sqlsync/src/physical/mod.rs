mod page;
mod storage;

pub use storage::Storage;
pub use page::SparsePages;

pub const PAGESIZE: usize = 4096;
