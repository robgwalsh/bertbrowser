//! Win32 `FILE_ATTRIBUTE_*` bits as the index, the search grammar and the listing use them.

pub const READONLY: u32 = 0x0001;
pub const HIDDEN: u32 = 0x0002;
pub const SYSTEM: u32 = 0x0004;
pub const DIRECTORY: u32 = 0x0010;
pub const ARCHIVE: u32 = 0x0020;
pub const NORMAL: u32 = 0x0080;
pub const TEMPORARY: u32 = 0x0100;
pub const SPARSE_FILE: u32 = 0x0200;
pub const REPARSE_POINT: u32 = 0x0400;
pub const COMPRESSED: u32 = 0x0800;
pub const OFFLINE: u32 = 0x1000;
pub const NOT_CONTENT_INDEXED: u32 = 0x2000;
pub const ENCRYPTED: u32 = 0x4000;
pub const INTEGRITY_STREAM: u32 = 0x8000;
pub const NO_SCRUB_DATA: u32 = 0x0002_0000;
pub const RECALL_ON_OPEN: u32 = 0x0004_0000;
pub const PINNED: u32 = 0x0008_0000;
pub const UNPINNED: u32 = 0x0010_0000;
pub const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
