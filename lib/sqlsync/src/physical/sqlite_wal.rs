use anyhow::{anyhow, Result};
use binary_layout::define_layout;
use byteorder::BigEndian;

use super::{sqlite_chksum::sqlite_chksum, PAGESIZE, layer::Layer};

pub struct SqliteWal {
    data: Vec<u8>,
}

impl SqliteWal {
    pub fn new() -> SqliteWal {
        SqliteWal { data: Vec::new() }
    }

    pub fn truncate(&mut self, size: usize) {
        self.data.truncate(size)
    }

    pub fn num_pages(&self) -> usize {
        // TODO: make this more robust to partially written WAL files
        // TODO: currently this takes advantage of the fact that we truncate
        // self.data on reset...  - this is not compat with regular sqlite WAL
        // files as they are not truncated and simply start writing from the
        // beginning and use salt values to detect where valid pages end

        if self.data.len() <= HEADER_SIZE {
            return 0;
        }

        // wal is arranged like so:
        // wal_header (HEADER_SIZE bytes)
        // page_0_header (page_header_size bytes)
        // page_0_data (PAGESIZE bytes)
        // ...
        let page_header_size = 24;

        // so to calculate total number of pages
        // we subtract the header size from the total size
        // and then divide by the size of a page + header

        // first assert the file matches our expectation
        assert_eq!(
            (self.data.len() - HEADER_SIZE) % (page_header_size + PAGESIZE),
            0
        );
        (self.data.len() - HEADER_SIZE) / (page_header_size + PAGESIZE)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize> {
        let current_len = self.data.len();
        let write_len = buf.len();
        let end = offset + write_len;

        if offset > current_len {
            // write start is out of range
            return Err(anyhow!("write start is out of range"));
        }

        if end > current_len {
            // write end is out of range
            self.data.resize(end, 0);
        }

        self.data[offset..end].copy_from_slice(buf);

        Ok(write_len)
    }

    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        let remaining = self.data.len().saturating_sub(offset);
        let n = remaining.min(buf.len());
        if n != 0 {
            buf[..n].copy_from_slice(&self.data[offset..offset + n]);
        }
        Ok(n)
    }

    pub fn reset(&mut self) {
        // read previous wal's salt1
        let prev_salt1 = header_layout::View::new(&self.data).salts().salt1().read();

        // create a new empty wal header
        let mut wal_hdr = header_layout::View::new([0u8; HEADER_SIZE]);

        // 0x377f0682 == BigEndian
        wal_hdr.magic_mut().write(0x377f0683);
        wal_hdr.file_format_write_version_mut().write(3007000);
        wal_hdr.page_size_mut().write(PAGESIZE as u32);
        wal_hdr.checkpoint_sequence_number_mut().write(0);
        let mut salts_view = wal_hdr.salts_mut();
        salts_view.salt1_mut().write(prev_salt1.wrapping_add(1));
        salts_view.salt2_mut().write(rand::random::<u32>());

        // calculate and store the wal checksum
        let wal_hdr = wal_hdr.into_storage();
        let (checksum1, checksum2) = sqlite_chksum::<BigEndian>(0, 0, &wal_hdr[0..24]);
        let mut wal_hdr = header_layout::View::new(wal_hdr);
        wal_hdr.checksum1_mut().write(checksum1);
        wal_hdr.checksum2_mut().write(checksum2);

        // truncate the wal to the new header length
        self.data.truncate(HEADER_SIZE);

        // write the new header
        self.data.copy_from_slice(&wal_hdr.into_storage());
    }

    pub fn chksum(&self) -> (u32, u32) {
        let hdr = header_layout::View::new(&self.data);
        (hdr.checksum1().read(), hdr.checksum2().read())
    }

    pub fn salts(&self) -> (u32, u32) {
        let hdr = header_layout::View::new(&self.data);
        let view = hdr.salts();
        (view.salt1().read(), view.salt1().read())
    }

    pub fn as_layer(&self) -> Layer {
        todo!()
    }
}

define_layout!(wal_salts, BigEndian, {
    salt1: u32,
    salt2: u32,
});

// sqlite wal header
define_layout!(header_layout, BigEndian, {
    // magic number
    magic: u32,
    // file format write version
    file_format_write_version: u32,
    // database page size
    page_size: u32,
    // checkpoint sequence number
    checkpoint_sequence_number: u32,
    // salts
    salts: wal_salts::NestedView,
    // checksum-1
    checksum1: u32,
    // checksum-2
    checksum2: u32,
});

pub const HEADER_SIZE: usize = match header_layout::SIZE {
    Some(size) => size,
    _ => panic!("header_layout::SIZE is not static"),
};
