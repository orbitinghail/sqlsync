use binary_layout::prelude::*;

// TODO: delete this if it's unused
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
