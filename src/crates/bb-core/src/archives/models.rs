//! What a container holds: the raw entries a reader reports, the tree they describe, and why a
//! container could not be listed. Spec: Core/Services/Archives/ArchiveModels.cs.
//!
//! **A bad archive is a message, never a panic.** `ArchiveIndex::failed` is how a damaged,
//! encrypted or oversized container arrives, which is the contract the preview pane's listing
//! already had and which now holds up a whole browsing surface.

use std::collections::HashMap;

use bb_paths::fold_upper;

use crate::FileTime;

/// One entry as the container reported it, before any tree is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawArchiveEntry {
    /// Raw entry key, separators as the container spelled them. None when the container gave none.
    pub key: Option<String>,
    /// Uncompressed length. Directories carry zero.
    pub size: i64,
    /// Stored length, or zero when the container does not say.
    pub compressed_size: i64,
    /// Modification time in UTC, or None when the container carries none.
    pub modified: Option<FileTime>,
    pub is_directory: bool,
    /// Whether this entry's data needs a password.
    pub is_encrypted: bool,
    /// Some for a symlink; never followed or recreated.
    pub link_target: Option<String>,
}

impl RawArchiveEntry {
    pub fn new(
        key: Option<&str>,
        size: i64,
        compressed_size: i64,
        modified: Option<FileTime>,
        is_directory: bool,
    ) -> RawArchiveEntry {
        RawArchiveEntry {
            key: key.map(str::to_string),
            size,
            compressed_size,
            modified,
            is_directory,
            is_encrypted: false,
            link_target: None,
        }
    }
}

/// What an opened container turned out to support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveCapabilities {
    /// Entries cannot be read individually: a solid archive, or a format with no index. Costs a
    /// full pass to list, turns preview-on-selection into a button, and makes any byte estimate a
    /// floor.
    pub sequential_only: bool,
    /// The container reported encryption on itself or on some entry.
    pub is_encrypted: bool,
    /// False for a truncated or multi-volume-with-missing-parts container.
    pub is_complete: bool,
}

impl ArchiveCapabilities {
    pub const UNKNOWN: ArchiveCapabilities = ArchiveCapabilities {
        sequential_only: false,
        is_encrypted: false,
        is_complete: true,
    };

    pub fn new(
        sequential_only: bool,
        is_encrypted: bool,
        is_complete: bool,
    ) -> ArchiveCapabilities {
        ArchiveCapabilities {
            sequential_only,
            is_encrypted,
            is_complete,
        }
    }
}

/// Why an archive could not be listed. The file list turns each into a banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ArchiveFailure {
    #[default]
    None,
    /// Not a container we can read, or the bytes are damaged.
    Damaged,
    /// Readable only with a password we do not have.
    PasswordRequired,
    /// More entries than the browse cap allows.
    TooManyEntries,
    /// The file could not be opened at all: gone, locked, or unreadable.
    Unreadable,
}

/// One node of the tree an archive's entries describe.
#[derive(Clone, Debug)]
pub struct ArchiveNode {
    /// Path within the archive, `\`-separated, no leading or trailing separator.
    pub path: String,
    /// Last segment of `path`.
    pub name: String,
    pub is_directory: bool,
    /// Uncompressed bytes: the entry's own for a file, the exact recursive sum for a directory.
    /// Never negative; inside an archive every size is known.
    pub size_bytes: i64,
    /// Stored bytes, summed the same way. Zero when the container does not report it.
    pub compressed_bytes: i64,
    /// None when the container carried no timestamp, which renders blank, never 1601.
    pub modified: Option<FileTime>,
    pub is_encrypted: bool,
    /// Some for a symlink entry. Listed, never followed.
    pub link_target: Option<String>,
    /// Arena indexes of the children, in name order once built.
    pub(crate) children: Vec<usize>,
}

impl ArchiveNode {
    pub(crate) fn directory(path: &str, name: &str) -> ArchiveNode {
        ArchiveNode {
            path: path.to_string(),
            name: name.to_string(),
            is_directory: true,
            size_bytes: 0,
            compressed_bytes: 0,
            modified: None,
            is_encrypted: false,
            link_target: None,
            children: Vec::new(),
        }
    }
}

/// An archive's entries as a directory tree, plus what reading it turned out to cost. Built once
/// per open and cached. Nodes live in one arena; `by_path` maps an upper-cased entry path to its
/// slot, and the root is always slot 0.
#[derive(Clone, Debug)]
pub struct ArchiveIndex {
    pub(crate) nodes: Vec<ArchiveNode>,
    pub(crate) by_path: HashMap<String, usize>,
    pub file_count: i32,
    pub capabilities: ArchiveCapabilities,
    /// Entries the container listed that this app refused: a key escaping the root, a missing key,
    /// or a name colliding with a directory. Reported, never silently zero.
    pub refused_count: i32,
    pub failure: ArchiveFailure,
    pub error: Option<String>,
}

impl ArchiveIndex {
    /// The browse cap, deliberately a different number from `ArchiveListing::DEFAULT_MAX_ENTRIES`
    /// (1,000). That one is a "while somebody arrows down a list" number and must stay small. This
    /// one is the largest directory the app already renders, at about a node plus a map slot each.
    /// Unifying them is how the preview pane starts costing 200,000 allocations per keypress.
    pub const MAX_ENTRIES: usize = 200_000;

    pub fn failed(failure: ArchiveFailure, message: &str) -> ArchiveIndex {
        ArchiveIndex {
            nodes: vec![ArchiveNode::directory("", "")],
            by_path: HashMap::new(),
            file_count: 0,
            capabilities: ArchiveCapabilities::UNKNOWN,
            refused_count: 0,
            failure,
            error: Some(message.to_string()),
        }
    }

    pub fn ok(&self) -> bool {
        self.failure == ArchiveFailure::None
    }

    pub fn root(&self) -> &ArchiveNode {
        &self.nodes[0]
    }

    /// How many entries the tree holds, root excluded.
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// The children of one directory inside the archive, or None when there is no such
    /// directory. An empty directory yields an empty list, which is not the same answer.
    pub fn children(&self, entry_path: &str) -> Option<Vec<&ArchiveNode>> {
        let node = self.find(entry_path)?;
        if !node.is_directory {
            return None;
        }
        Some(self.children_of(node))
    }

    /// The children of a node already in hand.
    pub fn children_of(&self, node: &ArchiveNode) -> Vec<&ArchiveNode> {
        node.children.iter().map(|&i| &self.nodes[i]).collect()
    }

    /// The node at `entry_path`, or None.
    pub fn find(&self, entry_path: &str) -> Option<&ArchiveNode> {
        let key = entry_path.trim_matches('\\');
        if key.is_empty() {
            return Some(self.root());
        }
        self.by_path.get(&fold_upper(key)).map(|&i| &self.nodes[i])
    }

    /// Every entry in the tree, root excluded, in no particular order.
    pub fn nodes(&self) -> impl Iterator<Item = &ArchiveNode> {
        self.by_path.values().map(|&i| &self.nodes[i])
    }
}
