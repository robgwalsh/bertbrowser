//! The NTFS master file table and USN journal: raw readers, the per-volume indexer, the directory
//! size builder and the helper host/client. Spec: Core/Services/Mft/*, Core/Interop/NtfsNative.cs.
//! (Only the reason constants exist yet; the readers arrive with M3.)

/// The slice of the MFT index service that search asks: whether volumes are still building (so
/// results are partial and a refresh is coming) and whether a root is kept live by the USN tail
/// (so it never falls to the crawler).
// TODO(port): extended when Mft (M3) is ported; the full IMftIndexService also has start/stop,
// presence, the building drives and the change-log policy.
pub trait MftIndex: Send + Sync {
    fn is_building(&self) -> bool;
    fn is_indexed(&self, path_key: &str) -> bool;
}

/// An index that indexes nothing and reports nothing: what a non-NTFS machine, a still-indexing one
/// and a declined elevation prompt all look like, so every caller downstream already handles it.
/// The harness and the tests use it; it can never raise an elevation prompt.
pub struct NullMftIndex;

impl MftIndex for NullMftIndex {
    fn is_building(&self) -> bool {
        false
    }

    fn is_indexed(&self, _path_key: &str) -> bool {
        false
    }
}

/// USN_RECORD reason bits the indexer and the change log act on.
pub mod usn_reason {
    pub const DATA_EXTEND: u32 = 0x0000_0002;
    pub const FILE_CREATE: u32 = 0x0000_0100;
    pub const FILE_DELETE: u32 = 0x0000_0200;
    pub const RENAME_OLD_NAME: u32 = 0x0000_1000;
    pub const RENAME_NEW_NAME: u32 = 0x0000_2000;
    pub const CLOSE: u32 = 0x8000_0000;
}
