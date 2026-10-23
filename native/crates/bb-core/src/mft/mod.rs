//! The NTFS master file table and USN journal: raw readers, the per-volume indexer, the directory
//! size builder and the helper host/client. Spec: Core/Services/Mft/*, Core/Interop/NtfsNative.cs.
//! (Only the reason constants exist yet; the readers arrive with M3.)

/// USN_RECORD reason bits the indexer and the change log act on.
pub mod usn_reason {
    pub const DATA_EXTEND: u32 = 0x0000_0002;
    pub const FILE_CREATE: u32 = 0x0000_0100;
    pub const FILE_DELETE: u32 = 0x0000_0200;
    pub const RENAME_OLD_NAME: u32 = 0x0000_1000;
    pub const RENAME_NEW_NAME: u32 = 0x0000_2000;
    pub const CLOSE: u32 = 0x8000_0000;
}
