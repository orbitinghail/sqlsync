use binary_layout::define_layout;
use byteorder::BigEndian;

use super::{
    sqlite_chksum::sqlite_chksum,
    sqlite_wal::{wal_salts, SqliteWal},
    PAGESIZE,
};

// we expect all shm allocations to be 16KB
const EXPECTED_REGION_SIZE: usize = 2 << 14;

pub struct SqliteShm {
    data: Vec<Vec<u8>>,
}

impl SqliteShm {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn shm_map(&mut self, region: usize, size: usize, create: bool) -> *const u8 {
        assert!(
            size == EXPECTED_REGION_SIZE,
            "unexpected shm_map size {}",
            size
        );

        if self.data.get(region).is_none() {
            assert!(
                region == self.data.len(),
                "unexpected shm_map region {}",
                region
            );
            if create {
                self.data.resize(region + 1, Vec::new());
            } else {
                // create is false, region doesn't exist, return null ptr
                return std::ptr::null();
            }
        }

        let data = &mut self.data[region];
        if data.is_empty() {
            data.resize(size, 0);
        }
        assert!(data.len() == size, "unexpected shm_map size {}", data.len());

        data.as_ptr()
    }

    pub fn shm_unmap(&mut self) {
        self.data.clear();
    }

    pub fn reset(&mut self, db_page_cnt: usize, wal: &SqliteWal) {
        // TODO: need to test this when the shm has more than one block

        let next_ichange = match self.data.get(0) {
            Some(shm) => {
                let hdr = header_layout::View::new(shm);
                hdr.ichange().read().wrapping_add(1)
            }
            None => 0,
        };
        let (wal_chksum1, wal_chksum2) = wal.chksum();
        let (wal_salt1, wal_salt2) = wal.salts();

        // create a new shm header info block
        let mut hdr = header_layout::View::new([0u8; HEADER_SIZE]);

        hdr.iversion_mut().write(3007000);
        hdr.ichange_mut().write(next_ichange);
        hdr.is_init_mut().write(1);
        hdr.big_endian_checksum_mut().write(1);
        hdr.page_size_mut().write(PAGESIZE as u16);
        hdr.max_frame_count_mut().write(0);
        hdr.database_size_mut().write(db_page_cnt as u32);
        hdr.last_frame_checksum1_mut().write(wal_chksum1);
        hdr.last_frame_checksum2_mut().write(wal_chksum2);
        hdr.salts_mut().salt1_mut().write(wal_salt1);
        hdr.salts_mut().salt2_mut().write(wal_salt2);

        // calculate and store the shm header checksum
        let hdr = hdr.into_storage();
        // TODO: is this supposed to be native endian?
        let (checksum1, checksum2) = sqlite_chksum::<BigEndian>(0, 0, &hdr[0..40]);
        let mut hdr = header_layout::View::new(hdr);
        hdr.checksum1_mut().write(checksum1);
        hdr.checksum2_mut().write(checksum2);

        // create a new shm header checkpoint block
        let info_hdr = hdr.into_storage();
        let checkpoint = [0u8; CHECKPOINT_SIZE];

        // create and initialize the full shm header
        let mut full_hdr = [0u8; (HEADER_SIZE * 2) + CHECKPOINT_SIZE];
        full_hdr[0..HEADER_SIZE].copy_from_slice(&info_hdr);
        full_hdr[HEADER_SIZE..HEADER_SIZE * 2].copy_from_slice(&info_hdr);
        full_hdr[HEADER_SIZE * 2..].copy_from_slice(&checkpoint);

        if self.data.is_empty() {
            let mut full_hdr = full_hdr.to_vec();
            full_hdr.resize(2 << 14, 0);
            self.data.push(full_hdr);
        } else {
            self.data.truncate(1);
            let shm = &mut self.data[0];
            shm[0..full_hdr.len()].copy_from_slice(&full_hdr);
            // zero rest of shm
            shm[full_hdr.len()..].fill(0);
        }
    }
}

// sqlite wal index header
define_layout!(header_layout, NativeEndian, {
    // wal-index format version number
    iversion: u32,
    // unused padding
    _unused: [u8; 4],
    // transaction counter
    ichange: u32,
    // isInit
    is_init: u8,
    // uses big-endian checksums
    big_endian_checksum: u8,
    // database page size
    page_size: u16,
    // number of valid and committed frames in the WAL
    max_frame_count: u32,
    // size of the database file in pages
    database_size: u32,
    // checksum of the last frame in the WAL
    last_frame_checksum1: u32,
    last_frame_checksum2: u32,
    // the two salt values copied from the WAL - in the byte order of the WAL (big-endian)
    salts: wal_salts::NestedView,
    // checksum
    checksum1: u32,
    checksum2: u32,
});

pub const HEADER_SIZE: usize = match header_layout::SIZE {
    Some(size) => size,
    _ => panic!("header_layout::SIZE is not static"),
};

define_layout!(checkpoint_layout, NativeEndian, {
    // Number of WAL frames backfilled into DB
    backfill: u32,
    // Reader marks
    readmark1: u32,
    readmark2: u32,
    readmark3: u32,
    readmark4: u32,
    readmark5: u32,
    // Reserved space for locks
    locks: [u8; 8],
    // WAL frames perhaps written, or maybe not
    backfill_attempted: u32,
    // Available for future enhancements
    _unused: u32,
});

pub const CHECKPOINT_SIZE: usize = match checkpoint_layout::SIZE {
    Some(size) => size,
    _ => panic!("checkpoint_layout::SIZE is not static"),
};
