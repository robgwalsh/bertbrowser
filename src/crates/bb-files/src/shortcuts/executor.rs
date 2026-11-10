//! Carries out a `ShortcutPlan`. Spec: Core/Services/Shortcuts/ShortcutExecutor.cs.
//!
//! The plan's link names are re-checked against live disk before anything is written. The plan was
//! built while a menu sat open under the cursor, and a name that was free then may not be now —
//! writing over it would destroy a file the user never named. This is the same re-check the
//! transfer, rename and new-item executors do, for the same reason. One item's failure never
//! affects the others: each link is written on its own and a refusal is collected rather than
//! propagated.

use std::collections::HashSet;
use std::io;

use bb_paths::{PathKey, UniquePath, file_name};

use super::planner::LOCAL_PROBE;
use super::{FailedShortcut, ShortcutOutcome, ShortcutPlan, ShortcutProbe};
use crate::fs::AccessDenied;

/// Writes one `.lnk`. The seam that keeps shell COM out of this crate, the way the file copier
/// keeps the copy loop swappable — the real implementation lives beside the other shell interop.
/// A COM refusal arrives as an `io::Error` carrying its code, so `AccessDenied` can read it.
pub trait ShortcutWriter {
    /// Creates `link_path` pointing at `target_path`. Fails on failure; "never overwrites
    /// silently" is the caller's rule, not this one's.
    fn write(&self, link_path: &str, target_path: &str) -> io::Result<()>;
}

pub struct ShortcutExecutor<'a> {
    writer: &'a dyn ShortcutWriter,
    probe: &'a dyn ShortcutProbe,
}

impl<'a> ShortcutExecutor<'a> {
    pub fn new(
        writer: &'a dyn ShortcutWriter,
        probe: &'a dyn ShortcutProbe,
    ) -> ShortcutExecutor<'a> {
        ShortcutExecutor { writer, probe }
    }

    /// An executor over the real disk.
    pub fn local(writer: &'a dyn ShortcutWriter) -> ShortcutExecutor<'a> {
        ShortcutExecutor {
            writer,
            probe: &LOCAL_PROBE,
        }
    }

    pub fn execute(&self, plan: &ShortcutPlan) -> ShortcutOutcome {
        if !plan.has_work() {
            return ShortcutOutcome::empty();
        }

        let mut created = Vec::new();
        let mut failed = Vec::new();

        // Links written by this run count as taken too: the plan reserved names against the disk
        // it saw, and a name it stepped aside from may itself have been freed since.
        let mut written: HashSet<String> = HashSet::new();

        for creation in &plan.creations {
            let link = UniquePath::for_path(
                &creation.link_path,
                false,
                |p| self.probe.directory_exists(p),
                |candidate| {
                    PathKey::canonicalize(candidate).is_ok_and(|k| written.contains(&k))
                        || self.probe.file_exists(candidate)
                },
            );

            match self.writer.write(&link, &creation.target_path) {
                Ok(()) => {
                    if let Ok(key) = PathKey::canonicalize(&link) {
                        written.insert(key);
                    }
                    created.push(link);
                }
                Err(e) => failed.push(FailedShortcut {
                    link_path: creation.link_path.clone(),
                    message: format!(
                        "Could not create a shortcut to '{}' \u{2014} {e}",
                        file_name(&creation.target_path)
                    ),
                    access_denied: AccessDenied::caused(Some(&e)),
                }),
            }
        }

        ShortcutOutcome { created, failed }
    }
}
