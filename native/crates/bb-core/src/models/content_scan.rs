//! What a `content:` reading pass did, for the status bar and for the honest account behind an
//! empty or short result. Spec: Core/Models/ContentScanModels.cs (`ContentMatch` itself is in
//! `crate::search`, beside the snippet that builds it).

/// Live progress of the reading pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentScanProgress {
    /// Files opened so far.
    pub files_read: usize,
    /// How many the pass expects to open.
    pub files_to_read: usize,
    /// Bytes read so far.
    pub bytes_read: u64,
}

/// Which ceiling a content search ran into, if any.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentScanLimit {
    /// It finished: every candidate was examined.
    #[default]
    None,
    /// The first pass filled up, so files past it were never considered.
    Candidates,
    /// The file-open ceiling was reached.
    Files,
    /// The byte ceiling was reached.
    Bytes,
}

/// What the reading pass actually did.
///
/// Modelled on the duplicate scan's outcome, which keeps "the user stopped it" and "something could
/// not be read" apart: conflating them makes a cancelled run look like a disk full of broken files.
/// `last_path_examined` is the field worth defending: a whole-PC content search scans the index in
/// path order, so hitting the candidate ceiling means stopping somewhere alphabetical, very possibly
/// before `C:\Users`. "Searched the first 50,000 files" is true and still reads as "your PC has no
/// such file"; naming where it got to is the difference between a bound and a lie.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentScanReport {
    /// How many files were opened and searched.
    pub files_read: usize,
    /// How many bytes of them were read.
    pub bytes_read: u64,
    /// Which ceiling stopped it, if any.
    pub limit: ContentScanLimit,
    /// How many candidates could not be read at all.
    pub unreadable: usize,
    /// How many were longer than the per-file budget, so only their front was searched.
    pub truncated: usize,
    /// The last candidate considered, for saying where a ceiling landed.
    pub last_path_examined: Option<String>,
}

impl ContentScanReport {
    /// Whether the answer is a floor rather than the answer: something was skipped or cut short.
    pub fn incomplete(&self) -> bool {
        self.limit != ContentScanLimit::None || self.unreadable > 0 || self.truncated > 0
    }
}
