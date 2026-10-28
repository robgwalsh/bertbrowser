//! Turns a folder that clashes with a folder into the handful of items inside it that actually
//! clash. Spec: Core/Services/Transfer/TransferMergeExpander.cs.
//!
//! **Why this is a separate step rather than part of the planner.** The drop pipeline builds a
//! plan on every drag-over to decide what the cursor should say; a planner that opened directories
//! would hit the disk on hover. So the planner keeps taking the narrower `TransferProbe`, which
//! cannot enumerate, and this runs once, at the drop, off the UI thread.
//!
//! **What it emits.** Still one plan, with the same destination directory; the items are spread
//! across the merged subtree. The load-bearing part: **every emitted destination's parent already
//! exists**, because the walk descends only where *both* sides have a real directory, so nothing
//! here ever has to invent a folder nobody asked for.
//!
//! **A link is always a leaf.** The source rule is the ordinary one. The destination rule matters
//! more: a junction at the destination pointing back into the source would merge a folder into
//! itself, one file at a time, by a path `Revalidate` never inspects, since it only compares a
//! source against the plan's destination directory and an expanded source is always deeper.

use std::collections::{HashMap, HashSet};

use bb_paths::fold_upper;

use super::models::{
    ClashSide, PlannedTransfer, TransferClash, TransferPlan, join, key_of, parent_of,
};
use super::probe::{FileSystemTransferProbe, TransferEntry, TransferEntrySource};
use crate::cancel::{CancelToken, Cancelled};

/// The ceilings one expansion works within.
pub struct TransferMergeLimits;

impl TransferMergeLimits {
    /// How many items one folder may expand into. Every one becomes a row in the progress window
    /// and, if it clashes, a row of radio buttons in a dialog. Ten thousand questions is not a
    /// question anyone can answer; past some size the wholesale question is the more useful one.
    pub const MAX_EXPANDED_TRANSFERS: usize = 5_000;

    /// How many directories one expansion may open. Bounds the *time* as well as the result, so a
    /// pathological overlap cannot spend a minute enumerating before discovering it is too big.
    pub const MAX_DIRECTORIES_VISITED: usize = 2_000;
}

pub struct TransferMergeExpander {
    source: Box<dyn TransferEntrySource>,
}

impl Default for TransferMergeExpander {
    fn default() -> Self {
        TransferMergeExpander::new(Box::new(FileSystemTransferProbe))
    }
}

/// Directory listings already read, by directory. A drop's sources usually share one parent, and
/// the destination folder is asked about once per top-level clash.
type Listings = HashMap<String, HashMap<String, TransferEntry>>;

struct Expansion {
    transfers: Vec<PlannedTransfer>,
    opened: Vec<String>,
}

impl TransferMergeExpander {
    pub fn new(source: Box<dyn TransferEntrySource>) -> TransferMergeExpander {
        TransferMergeExpander { source }
    }

    pub fn expand(
        &self,
        plan: TransferPlan,
        cancel: &CancelToken,
    ) -> Result<TransferPlan, Cancelled> {
        if plan.conflicts().is_empty() {
            return Ok(plan);
        }

        let mut listings: Listings = HashMap::new();
        let mut transfers = Vec::with_capacity(plan.transfers.len());
        let mut claimed: HashSet<String> = HashSet::new();
        let mut prune_directories = Vec::new();
        let mut merged_folders = Vec::new();

        for transfer in &plan.transfers {
            cancel.check()?;

            let existing = self
                .at(
                    &mut listings,
                    Some(&plan.destination_directory),
                    transfer.name(),
                )
                .cloned();

            if !self.mergeable(&mut listings, transfer, existing.as_ref()) {
                transfers.push(self.whole(
                    &mut listings,
                    transfer,
                    existing.as_ref(),
                    &mut claimed,
                    false,
                ));
                continue;
            }

            match self.walk(&mut listings, transfer, &mut claimed, cancel)? {
                Some(merged) => {
                    transfers.extend(merged.transfers);
                    prune_directories.extend(merged.opened);
                    merged_folders.push(transfer.name().to_string());
                }
                None => {
                    // Too big to put item by item. It keeps the wholesale question it always had,
                    // and contributes nothing to the merge: no prune list, and not a merged folder.
                    transfers.push(self.whole(
                        &mut listings,
                        transfer,
                        existing.as_ref(),
                        &mut claimed,
                        true,
                    ));
                }
            }
        }

        // Deepest first. A child's full path is always longer than its parent's, so ordering by
        // length is enough to guarantee a folder is emptied before it is looked at.
        prune_directories.sort_by_key(|d| std::cmp::Reverse(d.len()));

        Ok(TransferPlan {
            transfers,
            prune_directories,
            merged_folders,
            ..plan
        })
    }

    /// A folder landing on a folder, with no link on either side. Everything else (a folder onto a
    /// file, a file onto a folder, a file onto a file, or either side a junction) stays one clash
    /// with one answer, exactly as before.
    fn mergeable(
        &self,
        listings: &mut Listings,
        transfer: &PlannedTransfer,
        existing: Option<&TransferEntry>,
    ) -> bool {
        transfer.conflicts
            && transfer.is_directory
            && existing.is_some_and(|e| e.is_directory && !e.is_reparse_point)
            && !self
                .at(
                    listings,
                    parent_of(&transfer.source_path).as_deref(),
                    transfer.name(),
                )
                .is_some_and(|e| e.is_reparse_point)
    }

    /// Walks the overlap. None when it hit a ceiling.
    fn walk(
        &self,
        listings: &mut Listings,
        folder: &PlannedTransfer,
        claimed: &mut HashSet<String>,
        cancel: &CancelToken,
    ) -> Result<Option<Expansion>, Cancelled> {
        let mut emitted = Vec::new();
        let mut opened: Vec<String> = Vec::new();
        let mut taken: HashSet<String> = HashSet::new();

        let mut pending = vec![(folder.source_path.clone(), folder.destination_path.clone())];

        while let Some((source_directory, destination_directory)) = pending.pop() {
            cancel.check()?;
            if opened.len() == TransferMergeLimits::MAX_DIRECTORIES_VISITED {
                return Ok(None);
            }

            // Opened rather than moved whole, so a move will have emptied it by the end.
            opened.push(source_directory.clone());

            for child in self.source.entries(&source_directory) {
                cancel.check()?;

                let destination_path = join(&destination_directory, &child.name);
                let twin = self
                    .at(listings, Some(&destination_directory), &child.name)
                    .cloned();

                // Both sides a real directory: nothing to ask about the folder itself, so open it.
                if child.is_directory
                    && !child.is_reparse_point
                    && twin
                        .as_ref()
                        .is_some_and(|t| t.is_directory && !t.is_reparse_point)
                {
                    pending.push((child.full_path.clone(), destination_path));
                    continue;
                }

                if emitted.len() == TransferMergeLimits::MAX_EXPANDED_TRANSFERS {
                    return Ok(None);
                }

                // Two folders of the same name merging into one destination can both reach the
                // same path, and the planner's own reservations covered only the top level.
                let key = key_of(&destination_path);
                let already_spoken_for = !taken.insert(key.clone()) || claimed.contains(&key);
                let conflicts = twin.is_some() || already_spoken_for;

                emitted.push(PlannedTransfer {
                    source_path: child.full_path.clone(),
                    is_directory: child.is_directory,
                    destination_path,
                    conflicts,
                    known_bytes: if child.is_directory {
                        None
                    } else {
                        Some(child.size_bytes)
                    },
                    clash: if conflicts {
                        Some(Self::clash(Self::side(&child), twin.as_ref()))
                    } else {
                        None
                    },
                    merge_declined: false,
                });
            }
        }

        claimed.extend(taken);
        Ok(Some(Expansion {
            transfers: emitted,
            opened,
        }))
    }

    /// A clash the expander left whole, described so the dialog need not stat it.
    fn whole(
        &self,
        listings: &mut Listings,
        transfer: &PlannedTransfer,
        existing: Option<&TransferEntry>,
        claimed: &mut HashSet<String>,
        declined: bool,
    ) -> PlannedTransfer {
        claimed.insert(key_of(&transfer.destination_path));
        if !transfer.conflicts {
            return transfer.clone();
        }

        // The incoming side comes from its own parent's listing, so a folder the merge declined is
        // described as fully as one it expanded: same size and timestamp, from the same source.
        let incoming = match self.at(
            listings,
            parent_of(&transfer.source_path).as_deref(),
            transfer.name(),
        ) {
            Some(entry) => Self::side(entry),
            None => ClashSide {
                is_directory: transfer.is_directory,
                bytes: transfer.known_bytes.unwrap_or(0),
                modified_utc: crate::FileTime::UNKNOWN,
            },
        };

        PlannedTransfer {
            clash: Some(Self::clash(incoming, existing)),
            merge_declined: declined,
            ..transfer.clone()
        }
    }

    fn clash(incoming: ClashSide, existing: Option<&TransferEntry>) -> TransferClash {
        match existing {
            Some(twin) => TransferClash::between(incoming, Self::side(twin)),
            None => TransferClash::arriving(incoming),
        }
    }

    fn side(entry: &TransferEntry) -> ClashSide {
        ClashSide {
            is_directory: entry.is_directory,
            bytes: entry.size_bytes,
            modified_utc: entry.modified_utc,
        }
    }

    /// One named entry of a directory, or None. Listings are read once and kept.
    fn at<'l>(
        &self,
        listings: &'l mut Listings,
        directory: Option<&str>,
        name: &str,
    ) -> Option<&'l TransferEntry> {
        let directory = directory?;
        let by_name = listings.entry(fold_upper(directory)).or_insert_with(|| {
            self.source
                .entries(directory)
                .into_iter()
                .map(|e| (fold_upper(&e.name), e))
                .collect()
        });
        by_name.get(&fold_upper(name))
    }
}
