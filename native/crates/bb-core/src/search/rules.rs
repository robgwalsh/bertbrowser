//! What a content search is allowed to spend. Ceilings, not targets: a query that reaches one still
//! returns everything it found and says which ceiling it hit. Measured against a real 245,305-file
//! tree: an 8 KB head sniff runs at 15,609 files/s on four threads, whole text files at ~49 MB/s,
//! and matching itself is free. Spec: Core/Services/Search/ContentSearchRules.cs.

pub struct ContentSearchRules;

impl ContentSearchRules {
    /// How much of a file is read before deciding whether it is text at all, and what the encoding is
    /// decided from: a binary then costs 8 KB instead of a megabyte.
    pub const HEAD_SAMPLE_BYTES: usize = 8 * 1024;
    /// How much of any one file is searched: the preview pane's text budget, deliberately.
    pub const MAX_BYTES_PER_FILE: u64 = 1 << 20;
    /// How many first-pass candidates a content query asks the index or the walk for.
    pub const MAX_CANDIDATES: usize = 50_000;
    /// How many files one search will open. A separate population from the candidates: a candidate
    /// the name already settled is never opened.
    pub const MAX_FILES_OPENED: usize = 50_000;
    /// And how many bytes of them, whichever ceiling is reached first.
    pub const MAX_BYTES_READ: u64 = 512 * 1024 * 1024;
    /// How many files are read at once. Four, measured; past this a spinning disk thrashes.
    pub const MAX_PARALLELISM: usize = 4;
}
