//! What a transfer is made of: the verb, the plan, its items, the outcome and the progress report.
//! Spec: Core/Services/Transfer/TransferModels.cs.

use std::collections::HashMap;
use std::fmt;
use std::io;

use bb_paths::{PathKey, parent_key};

use crate::FileTime;
use crate::fs::{AccessDenied, EntryStamp};

/// What a drop does with its sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferVerb {
    /// Relocate the sources. Destructive to the source location, so it is undoable.
    Move,
    /// Duplicate the sources. Purely additive: it never removes or overwrites anything, which is
    /// why `Replace` is not offered for it.
    Copy,
}

/// Why the planner refused to transfer one source. Everything except `MovesWithAncestor` and
/// `AlreadyInDestination` is a real problem worth telling the user about; those two are ordinary
/// no-ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferRejection {
    /// The source disappeared between selection and drop.
    SourceMissing,
    /// Drive roots and volume roots cannot be relocated.
    SourceIsRoot,
    /// The destination does not exist.
    DestinationMissing,
    /// The destination path is a file, not a folder.
    DestinationNotDirectory,
    /// Dropping a folder onto itself.
    DestinationIsSource,
    /// Dropping a folder into its own subtree: the case that eats directory trees.
    DestinationInsideSource,
    /// A move whose source already sits in the destination folder: nothing to do.
    AlreadyInDestination,
    /// An ancestor of this source is also being transferred, so it travels along with it.
    /// Transferring both would leave a dangling path once the ancestor has moved.
    MovesWithAncestor,
}

/// How to settle a name that already exists in the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConflictResolution {
    /// Leave the source where it is.
    Skip,
    /// Transfer under a generated "name (2)" style name. Never touches the existing entry.
    KeepBoth,
    /// Take over the existing name. The displaced entry is moved to a staging folder rather than
    /// deleted, so an undo can put it back. Move only.
    Replace,
    /// Take over the existing name on a **copy**. Writes exactly what `Replace` writes, through the
    /// same staging folder. What separates it is the undo: a copy's outcome is not undoable
    /// (`TransferOutcome::can_undo`), so a copy that displaced something would leave that entry in a
    /// hidden folder with no record pointing at it. Only a caller that keeps the outcome and can
    /// undo it may ask for this: a folder sync, and a merged drop or paste (`can_undo_copy`).
    Overwrite,
}

/// One side of a clash, as whatever found it already described it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClashSide {
    pub is_directory: bool,
    pub bytes: i64,
    /// Unknown (zero) when nothing knew it.
    pub modified_utc: FileTime,
}

/// Both sides of one name clash, and the two things a user needs told about it. **Carried on the
/// plan so the dialog does no disk IO**: one clash could afford a stat on the UI thread, a merged
/// folder's worth cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferClash {
    pub incoming: ClashSide,
    /// What is already at the destination, or None when the clash is with another item arriving in
    /// this same transfer, where nothing is on disk to describe.
    pub existing: Option<ClashSide>,
    /// Same size, same instant: replacing it would write the bytes that are already there, and
    /// keeping both would manufacture a second copy of a file the user already has.
    pub identical: bool,
    /// The common reason for wanting Replace, and what the dialog highlights.
    pub incoming_is_newer: bool,
}

/// FAT and exFAT round a write time to this.
const FAT_GRANULARITY_TICKS: i64 = 2 * crate::time::TICKS_PER_SECOND;

/// `CompareEquality.CompareTimes` at `CompareTolerance.Strict`: -1, 0 or +1, with zero meaning
/// "the same instant as far as two-second granularity can tell".
// TODO(port): extended when Compare is ported (the whole-hour `Loose` tolerance lives there).
fn compare_times_strict(a: FileTime, b: FileTime) -> i32 {
    let gap = a.ticks_since(b);
    if gap.abs() <= FAT_GRANULARITY_TICKS {
        0
    } else if gap > 0 {
        1
    } else {
        -1
    }
}

impl TransferClash {
    /// A clash with something on disk, judged. **Strict tolerance, never Loose.** The folder
    /// comparison forgives a whole-hour shift for FAT volumes; here that would pre-select Skip on a
    /// file that is genuinely an hour newer, so the user asks for a merge and silently does not get
    /// one. The two-second granularity is kept, because a file copied onto a USB stick really does
    /// come back up to two seconds off.
    pub fn between(incoming: ClashSide, existing: ClashSide) -> TransferClash {
        let comparable = !incoming.is_directory
            && !existing.is_directory
            && !incoming.modified_utc.is_unknown()
            && !existing.modified_utc.is_unknown();
        let by_time = if comparable {
            compare_times_strict(incoming.modified_utc, existing.modified_utc)
        } else {
            0
        };
        TransferClash {
            incoming,
            existing: Some(existing),
            identical: comparable && by_time == 0 && incoming.bytes == existing.bytes,
            incoming_is_newer: comparable && by_time > 0,
        }
    }

    /// A clash with another item arriving in this same transfer.
    pub fn arriving(incoming: ClashSide) -> TransferClash {
        TransferClash {
            incoming,
            existing: None,
            identical: false,
            incoming_is_newer: false,
        }
    }
}

/// What a clash should be answered with before anyone has said otherwise. In Core, used by both
/// the dialog and the headless prompt, so an unattended path and a visible one cannot start from
/// different answers.
pub struct ConflictDefaults;

impl ConflictDefaults {
    /// Keep both, except where the two sides are the same file: there, Skip. Keep both is the one
    /// answer that can neither lose the incoming copy nor displace what is already there; applied to
    /// a byte-identical file it manufactures `img (2).jpg` beside an `img.jpg` the user already has.
    pub fn for_transfer(transfer: &PlannedTransfer) -> ConflictResolution {
        match transfer.clash {
            Some(clash) if clash.identical => ConflictResolution::Skip,
            _ => ConflictResolution::KeepBoth,
        }
    }

    /// Whether Replace may be offered for this item at all. A move always could; a copy could not,
    /// because `can_undo` is false for one. Inside a merge the caller keeps the outcome and can
    /// reverse it, so Replace is offered; a folder too large to merge keeps the capabilities it had.
    pub fn allows_replace(plan: &TransferPlan, transfer: &PlannedTransfer) -> bool {
        plan.verb == TransferVerb::Move || (plan.is_merged() && !transfer.merge_declined)
    }

    /// What a chosen "Replace" actually means here: a copy says `Overwrite`, which the executor
    /// honours, where a plain `Replace` on a copy is still rewritten to Keep both.
    pub fn replace_means(plan: &TransferPlan) -> ConflictResolution {
        if plan.verb == TransferVerb::Move {
            ConflictResolution::Replace
        } else {
            ConflictResolution::Overwrite
        }
    }
}

/// One source the planner accepted, with the name it would land under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTransfer {
    /// Full path of the item to transfer.
    pub source_path: String,
    pub is_directory: bool,
    /// Where it lands when nothing is in the way.
    pub destination_path: String,
    /// True when `destination_path` is already taken.
    pub conflicts: bool,
    /// The source's size, when whatever produced this item already knew it. Set only by the merge
    /// expander, whose enumeration gets a file's length free from the find data. None everywhere
    /// else, and for a directory, which is sized from the index rather than from disk.
    pub known_bytes: Option<i64>,
    /// Both sides of this item's clash, when something already knew them. None on a plan that never
    /// went through the merge expander.
    pub clash: Option<TransferClash>,
    /// This folder clashes with a folder and would have been merged, but the overlap was too large
    /// to put to the user item by item. It keeps the whole-folder question it always had.
    pub merge_declined: bool,
}

impl PlannedTransfer {
    pub fn new(
        source_path: &str,
        is_directory: bool,
        destination_path: &str,
        conflicts: bool,
    ) -> PlannedTransfer {
        PlannedTransfer {
            source_path: source_path.to_string(),
            is_directory,
            destination_path: destination_path.to_string(),
            conflicts,
            known_bytes: None,
            clash: None,
            merge_declined: false,
        }
    }

    pub fn name(&self) -> &str {
        bb_paths::file_name(&self.source_path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedTransfer {
    pub source_path: String,
    pub reason: TransferRejection,
    /// User-facing explanation.
    pub message: String,
}

impl RejectedTransfer {
    pub fn new(source_path: &str, reason: TransferRejection, message: String) -> RejectedTransfer {
        RejectedTransfer {
            source_path: source_path.to_string(),
            reason,
            message,
        }
    }

    /// True for refusals that are ordinary no-ops rather than something to report.
    pub fn is_benign(&self) -> bool {
        matches!(
            self.reason,
            TransferRejection::AlreadyInDestination | TransferRejection::MovesWithAncestor
        )
    }
}

/// The validated outcome of asking "what would dropping these here do?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferPlan {
    pub verb: TransferVerb,
    pub destination_directory: String,
    pub transfers: Vec<PlannedTransfer>,
    pub rejected: Vec<RejectedTransfer>,
    /// Source directories the merge expander opened rather than moved whole, deepest first. A move
    /// removes each one that is empty when the run ends; a copy leaves them all. Empty on every
    /// plan that was not expanded, and none of those should prune.
    pub prune_directories: Vec<String>,
    /// The names of the folders this plan merges into a folder of the same name. Non-empty is what
    /// "this plan was expanded" means.
    pub merged_folders: Vec<String>,
}

impl TransferPlan {
    pub fn new(
        verb: TransferVerb,
        destination_directory: &str,
        transfers: Vec<PlannedTransfer>,
        rejected: Vec<RejectedTransfer>,
    ) -> TransferPlan {
        TransferPlan {
            verb,
            destination_directory: destination_directory.to_string(),
            transfers,
            rejected,
            prune_directories: Vec::new(),
            merged_folders: Vec::new(),
        }
    }

    pub fn empty(verb: TransferVerb, destination: &str) -> TransferPlan {
        TransferPlan::new(verb, destination, Vec::new(), Vec::new())
    }

    pub fn is_merged(&self) -> bool {
        !self.merged_folders.is_empty()
    }

    /// True when the drop would actually transfer something: the gate for allowing a drop.
    pub fn has_work(&self) -> bool {
        !self.transfers.is_empty()
    }

    pub fn conflicts(&self) -> Vec<&PlannedTransfer> {
        self.transfers.iter().filter(|t| t.conflicts).collect()
    }

    /// The earlier item in this same plan that already claimed `transfer`'s destination name, or
    /// None when nothing here did. **A clash is not always with something on disk**: two sources
    /// from different folders can carry the same name, and at planning time nothing is at that
    /// destination yet, so asking disk answers "missing". What the user needs to be told is which
    /// other item in this drop they are choosing between. `transfer` is identified by address, as
    /// the C# compared references.
    pub fn earlier_claimant_of(&self, transfer: &PlannedTransfer) -> Option<&PlannedTransfer> {
        let key = key_of(&transfer.destination_path);
        for other in &self.transfers {
            if std::ptr::eq(other, transfer) {
                break;
            }
            if key_of(&other.destination_path) == key {
                return Some(other);
            }
        }
        None
    }

    /// How to name an item to the user: its path relative to where the drop lands. A leaf name for
    /// everything the planner produced, since those all land directly in the destination folder;
    /// an expanded merge is what needs the rest, because forty rows called `config.json` are forty
    /// rows the user cannot tell apart.
    pub fn label_for(&self, transfer: &PlannedTransfer) -> String {
        let (Ok(base), Ok(path)) = (
            PathKey::normalize_display(&self.destination_directory),
            PathKey::normalize_display(&transfer.destination_path),
        ) else {
            return transfer.name().to_string();
        };
        let base_key = bb_paths::fold_upper(&base);
        let path_key = bb_paths::fold_upper(&path);
        if path_key == base_key {
            return ".".to_string();
        }
        if PathKey::is_under_key(&path_key, &base_key) {
            let body = base_key.strip_suffix('\\').unwrap_or(&base_key);
            return path[body.len() + 1..].to_string();
        }
        transfer.name().to_string()
    }

    /// Refusals worth surfacing; no-ops are filtered out.
    pub fn problems(&self) -> Vec<&RejectedTransfer> {
        self.rejected.iter().filter(|r| !r.is_benign()).collect()
    }
}

/// One source that made it onto disk at its new location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedTransfer {
    /// Where it came from: the undo target.
    pub source_path: String,
    /// Where it ended up, after any conflict resolution.
    pub final_path: String,
    pub is_directory: bool,
    /// When the transfer replaced an existing entry, the staging path that entry was moved aside
    /// to. Undo restores from here.
    pub displaced_stage_path: Option<String>,
    /// The copy as it was written, taken the moment it landed. Set only for a copy: `undo_copies`
    /// removes nothing that no longer matches it, so an entry edited since the paste, or the
    /// original a previous undo already put back under this name, is left alone rather than erased.
    /// None on a move, and on a completion that arrived over the elevation pipe before
    /// `stamp_copies` ran.
    pub written: Option<EntryStamp>,
}

/// The source that could not be transferred, and why, phrased for the status bar. `access_denied`
/// is the one failure an administrator token could fix, and therefore the only one the elevated
/// retry is ever offered for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedTransfer {
    pub source_path: String,
    pub message: String,
    pub access_denied: bool,
}

impl FailedTransfer {
    pub fn new(source_path: &str, message: String) -> FailedTransfer {
        FailedTransfer {
            source_path: source_path.to_string(),
            message,
            access_denied: false,
        }
    }

    pub fn from_error(source_path: &str, message: String, error: &TransferError) -> FailedTransfer {
        FailedTransfer {
            source_path: source_path.to_string(),
            message,
            access_denied: error.is_access_denied(),
        }
    }
}

/// What actually happened on disk. `completed` doubles as the undo record.
///
/// A single run only ever creates one staging folder, but an outcome is not always a single run:
/// the elevated retry merges the pass that failed on permissions with the pass that did not, and a
/// dropped second folder would never be committed and never purged. Hence a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferOutcome {
    pub verb: TransferVerb,
    pub destination_directory: String,
    pub completed: Vec<CompletedTransfer>,
    pub skipped: Vec<String>,
    pub failed: Vec<FailedTransfer>,
    /// Every staging folder this run created, so the whole lot can be discarded in one go once the
    /// transfer can no longer be undone.
    pub staging_directories: Vec<String>,
    /// True when the user stopped the transfer part-way. Without it a cancelled run is
    /// indistinguishable from an empty plan.
    pub cancelled: bool,
    /// Source directories this run removed because a merge had emptied them, deepest first. Undo
    /// recreates exactly these, and nothing else, before putting anything back, so the "its
    /// original folder is gone" guard keeps meaning *the user* deleted it.
    pub pruned_directories: Vec<String>,
    /// A copy whose writes can be taken back through `undo_copies`. Nothing in Core ever sets this:
    /// it is an undertaking by the caller to keep the outcome.
    pub can_undo_copy: bool,
}

impl TransferOutcome {
    pub fn new(
        verb: TransferVerb,
        destination_directory: &str,
        completed: Vec<CompletedTransfer>,
        skipped: Vec<String>,
        failed: Vec<FailedTransfer>,
        staging_directories: Vec<String>,
        cancelled: bool,
    ) -> TransferOutcome {
        TransferOutcome {
            verb,
            destination_directory: destination_directory.to_string(),
            completed,
            skipped,
            failed,
            staging_directories,
            cancelled,
            pruned_directories: Vec::new(),
            can_undo_copy: false,
        }
    }

    /// Only a move is worth undoing: a copy adds without removing or overwriting. A cancelled move
    /// still undoes: what got across is what goes back.
    pub fn can_undo(&self) -> bool {
        self.verb == TransferVerb::Move && !self.completed.is_empty()
    }
}

/// Progress while a transfer runs. Byte-level, because item counts say nothing useful about a
/// single large file. The plan's byte total is deliberately absent: it comes from the directory
/// size index, which is the caller's business.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    /// Items finished.
    pub done: usize,
    /// Items in the plan.
    pub total: usize,
    /// The item in flight; empty on the terminal report.
    pub current_name: String,
    /// Bytes written so far across the whole plan. A move within one volume is a rename, so it
    /// moves no bytes and this stays at zero, which is correct, not a stall.
    pub bytes_done: i64,
    /// Bytes written into the item in flight.
    pub current_item_bytes: i64,
    /// That item's size as the OS reports it; 0 when not yet known.
    pub current_item_total: i64,
}

/// How to settle conflicts, keyed by `PathKey::canonicalize` of the source path.
pub type Resolutions = HashMap<String, ConflictResolution>;

/// Why one item did not get across. `Cancelled` ends the run; everything else fails that item and
/// the run goes on, which is "one item's failure never affects the others".
#[derive(Debug)]
pub enum TransferError {
    Cancelled,
    Io(io::Error),
}

impl TransferError {
    pub fn other(message: impl Into<String>) -> TransferError {
        TransferError::Io(io::Error::other(message.into()))
    }

    pub fn not_found(message: impl Into<String>) -> TransferError {
        TransferError::Io(io::Error::new(io::ErrorKind::NotFound, message.into()))
    }

    /// The failure, as the user reads it: the OS text without the "(os error N)" suffix.
    pub fn message(&self) -> String {
        match self {
            TransferError::Cancelled => "cancelled".to_string(),
            TransferError::Io(e) => io_message(e),
        }
    }

    pub fn is_access_denied(&self) -> bool {
        match self {
            TransferError::Cancelled => false,
            TransferError::Io(e) => AccessDenied::caused(Some(e)),
        }
    }
}

impl From<io::Error> for TransferError {
    fn from(e: io::Error) -> Self {
        TransferError::Io(e)
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for TransferError {}

/// An `io::Error`'s text as `Exception.Message` read: the OS message alone, with the
/// "(os error N)" the standard library appends taken off.
pub fn io_message(e: &io::Error) -> String {
    let text = e.to_string();
    match (e.raw_os_error(), text.rfind(" (os error ")) {
        (Some(_), Some(at)) => text[..at].trim_end().to_string(),
        _ => text,
    }
}

/// `PathKey::canonicalize`, falling back to a plain upper-casing for a path Windows will not
/// resolve, as the C# did: an unusable key compares unequal to everything rather than failing.
pub(crate) fn key_of(path: &str) -> String {
    PathKey::canonicalize(path).unwrap_or_else(|_| bb_paths::fold_upper(path))
}

/// `Path.GetDirectoryName(Path.GetFullPath(path))`: None for a drive or volume root, which has no
/// parent to move out of, and for a path Windows cannot resolve.
pub(crate) fn parent_of(path: &str) -> Option<String> {
    let full = PathKey::normalize_display(path).ok()?;
    parent_key(&full)
}

/// `Path.Combine(directory, name)` for a directory that may or may not end in a separator.
pub(crate) fn join(directory: &str, name: &str) -> String {
    if directory.ends_with(['\\', '/']) {
        format!("{directory}{name}")
    } else {
        format!("{directory}\\{name}")
    }
}
