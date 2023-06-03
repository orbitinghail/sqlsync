use binary_layout::prelude::*;
use byteorder::ByteOrder;

define_layout!(sqlite3_header, BigEndian, {
    // The header string: "SQLite format 3\0".
    magic: [u8;16],
    // The database page size in bytes. Must be a power of two between 512 and 32768 inclusive, or the value 1 representing a page size of 65536.
    page_size: u16,
    // File format write version. 1 for legacy; 2 for WAL.
    file_format_write_version: u8,
    // File format read version. 1 for legacy; 2 for WAL.
    file_format_read_version: u8,
    // Bytes of unused "reserved" space at the end of each page. Usually 0.
    reserved_space: u8,
    // Maximum embedded payload fraction. Must be 64.
    max_payload_fraction: u8,
    // Minimum embedded payload fraction. Must be 32.
    min_payload_fraction: u8,
    // Leaf payload fraction. Must be 32.
    leaf_payload_fraction: u8,
    // File change counter.
    file_change_counter: u32,
    // Size of the database file in pages. The "in-header database size".
    database_size: u32,
    // Page number of the first freelist trunk page.
    first_freelist_trunk_page: u32,
    // Total number of freelist pages.
    total_freelist_pages: u32,
    // The schema cookie.
    schema_cookie: u32,
    // The schema format number. Supported schema formats are 1, 2, 3, and 4.
    schema_format_number: u32,
    // Default page cache size.
    default_page_cache_size: u32,
    // The page number of the largest root b-tree page when in auto-vacuum or incremental-vacuum modes, or zero otherwise.
    largest_root_btree_page_number: u32,
    // The database text encoding. A value of 1 means UTF-8. A value of 2 means UTF-16le. A value of 3 means UTF-16be.
    database_text_encoding: u32,
    // The "user version" as read and set by the user_version pragma.
    user_version: u32,
    // True (non-zero) for incremental-vacuum mode. False (zero) otherwise.
    incremental_vacuum_mode: u32,
    // The "Application ID" set by PRAGMA application_id.
    application_id: u32,
    // reserved for expansion, must be zero
    reserved: [u8; 20],
    // The version-valid-for number.
    version_valid_for: u32,
    // SQLITE_VERSION_NUMBER
    sqlite_version_number: u32,
});

// sqlite wal header
define_layout!(wal_header, BigEndian, {
    // magic number
    magic: u32,
    // file format write version
    file_format_write_version: u32,
    // database page size
    page_size: u32,
    // checkpoint sequence number
    checkpoint_sequence_number: u32,
    // salt-1
    salt1: u32,
    // salt-2
    salt2: u32,
    // checksum-1
    checksum1: u32,
    // checksum-2
    checksum2: u32,
});

pub const WAL_HEADER_SIZE: usize = match wal_header::SIZE {
    Some(size) => size,
    _ => panic!("wal_header size is not static"),
};

// sqlite wal index header
define_layout!(wal_index_header_info, BigEndian, {
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
    salt1: u32,
    salt2: u32,
    // checksum
    checksum1: u32,
    checksum2: u32,
});

pub const WAL_INDEX_HEADER_INFO_SIZE: usize = match wal_index_header_info::SIZE {
    Some(size) => size,
    _ => panic!("wal_index_header_info size is not static"),
};

define_layout!(wal_index_header_checkpoint, BigEndian, {
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

pub const WAL_INDEX_HEADER_CHECKPOINT_SIZE: usize = match wal_index_header_checkpoint::SIZE {
    Some(size) => size,
    _ => panic!("wal_index_header_checkpoint size is not static"),
};

// documented: https://sqlite.org/fileformat2.html#walcksm
pub fn wal_checksum<E: ByteOrder>(s0: u32, s1: u32, b: &[u8]) -> (u32, u32) {
    assert!(
        b.len() % 8 == 0,
        "wal_checksum: b.len() must be a multiple of 8"
    );

    let mut s0 = s0;
    let mut s1 = s1;
    for i in (0..b.len()).step_by(8) {
        s0 = s0.wrapping_add(s1.wrapping_add(E::read_u32(&b[i..])));
        s1 = s1.wrapping_add(s0.wrapping_add(E::read_u32(&b[i + 4..])));
    }
    (s0, s1)
}
