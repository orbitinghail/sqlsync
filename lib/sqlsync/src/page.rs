use std::{
    collections::BTreeMap,
    io::{self, Write},
    mem::size_of,
};

use crate::{positioned_io::PositionedReader, Serializable};

// TODO: profile both bandwidth usage and general perf for different page sizes on various workloads
// TODO: research OPFS block sizes and whether we should use that as a guide for page size
pub const PAGESIZE: usize = 4096;

/// PageIdx is the 1-based index of a page in a SQLite database file
pub type PageIdx = u32;
const PAGE_IDX_SIZE: usize = size_of::<PageIdx>();

pub type Page = [u8; PAGESIZE];

#[derive(Default, Debug, Clone)]
pub struct SparsePages {
    pages: BTreeMap<PageIdx, Page>,
}

impl SparsePages {
    pub fn new() -> SparsePages {
        Self { pages: BTreeMap::new() }
    }

    pub fn num_pages(&self) -> usize {
        self.pages.len()
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn write(&mut self, page_idx: PageIdx, page: Page) {
        self.pages.insert(page_idx, page);
    }

    pub fn page_idxs(&self) -> impl Iterator<Item = &PageIdx> {
        self.pages.keys()
    }

    // returns the max page index of this sparse pages object
    pub fn max_page_idx(&self) -> Option<PageIdx> {
        self.pages.keys().max().copied()
    }

    pub fn read(&self, page_idx: PageIdx, page_offset: usize, buf: &mut [u8]) -> usize {
        self.pages
            .get(&page_idx)
            .map(|page| {
                let end = page_offset + buf.len();
                assert!(end <= PAGESIZE, "page offset out of bounds");
                buf.copy_from_slice(&page[page_offset..end]);
                buf.len()
            })
            .unwrap_or(0)
    }
}

/// The serialized form of SparsePages can be read using the SerializedPagesReader object below
impl Serializable for SparsePages {
    fn serialize_into<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        assert!(
            !self.pages.is_empty(),
            "cannot serialize empty sparse pages obj"
        );

        // serialize the page indexes, sorted desc
        for page_idx in self.pages.keys().rev() {
            writer.write_all(&page_idx.to_le_bytes())?;
        }

        // serialize the pages, sorted by page_idx desc
        for page in self.pages.values().rev() {
            writer.write_all(&page[..])?;
        }

        Ok(())
    }
}

/// Binary layout of Serialized Page objects is:
/// for each page_idx (sorted desc) [
///   page_idx: u32
/// ]
/// for each page (sorted by page_idx desc) [
///   page: [u8; PAGESIZE]
/// ]
pub struct SerializedPagesReader<R: PositionedReader>(pub R);

impl<R: PositionedReader> SerializedPagesReader<R> {
    pub fn num_pages(&self) -> io::Result<usize> {
        let file_size = self.0.size()?;
        let num_pages = file_size / (PAGE_IDX_SIZE + PAGESIZE);
        Ok(num_pages)
    }

    pub fn max_page_idx(&self) -> io::Result<PageIdx> {
        let mut buf = [0; PAGE_IDX_SIZE];
        self.0.read_exact_at(0, &mut buf)?;
        Ok(PageIdx::from_le_bytes(buf))
    }

    // returns a list of page indexes contained by this serialized pages object
    // sorted desc
    pub fn page_idxs(&self) -> io::Result<Vec<PageIdx>> {
        let num_pages = self.num_pages()?;
        let mut buf = vec![0u8; PAGE_IDX_SIZE * num_pages];
        self.0.read_exact_at(0, &mut buf)?;

        Ok(buf
            .chunks_exact(PAGE_IDX_SIZE)
            .map(|chunk| PageIdx::from_le_bytes(chunk.try_into().unwrap()))
            .collect())
    }

    // binary searches for the page at the given page_idx, returning the offset
    // of the page in this file
    fn find_page_start(&self, page_idx: PageIdx) -> io::Result<Option<usize>> {
        let num_pages = self.num_pages()?;
        let mut left: usize = 0;
        let mut right: usize = num_pages;
        let mut page_idx_buf = [0; PAGE_IDX_SIZE];

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_offset = mid * PAGE_IDX_SIZE;
            self.0.read_exact_at(mid_offset, &mut page_idx_buf)?;

            let mid_idx = PageIdx::from_le_bytes(page_idx_buf);

            match mid_idx.cmp(&page_idx) {
                std::cmp::Ordering::Equal => {
                    let page_offset = (num_pages * PAGE_IDX_SIZE) + (mid * PAGESIZE);
                    return Ok(Some(page_offset));
                }
                std::cmp::Ordering::Less => {
                    // pages are sorted in descending order, so we need to search left
                    right = mid;
                }
                std::cmp::Ordering::Greater => {
                    // pages are sorted in descending order, so we need to search right
                    left = mid + 1;
                }
            }
        }

        Ok(None)
    }

    pub fn read(&self, page_idx: PageIdx, page_offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        assert!(page_offset < PAGESIZE, "page_offset must be < PAGESIZE");
        assert!(
            page_offset + buf.len() <= PAGESIZE,
            "refusing to read more than one page"
        );

        if let Some(page_start) = self.find_page_start(page_idx)? {
            let read_start = page_start + page_offset;
            self.0.read_exact_at(read_start, buf)?;
            Ok(buf.len())
        } else {
            Ok(0)
        }
    }
}
