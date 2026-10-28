//! Walks the history to a chosen entry ("undo to here", "redo to here") one step at a time.
//! Spec: Core/Services/UndoHistory/UndoNavigator.cs.
//!
//! **It stops at the first step that is not clean.** Each entry's operation may depend on the one
//! before it being undone completely: a rename undone after a move that only half went back would
//! rename whatever is standing at those names now. So a partial step is taken, reported, and the
//! walk goes no further: "undo 5" that trips on the third leaves the fourth and fifth done.

use super::host::UndoHost;
use super::models::{HeldSize, StepReport, UndoRefresh};
use super::record::UndoableRecord;
use super::stack::{EntryId, Released, UndoStack};
use crate::FileTime;

/// Where a walk through the history ended, and what it leaves for the caller to do.
pub struct NavigationResult {
    /// Steps that moved something, partial ones included.
    pub steps_taken: usize,
    /// The entry whose step was not clean, when one was.
    pub stopped_at: Option<EntryId>,
    /// That step's report.
    pub stop_report: Option<StepReport>,
    /// What every step taken leaves stale on screen.
    pub refreshes: Vec<UndoRefresh>,
    /// What the walk pushed out of the budget, for the caller to retire.
    pub released: Released,
    pub notes: Vec<String>,
}

pub struct UndoNavigator;

impl UndoNavigator {
    /// `measure` is how much a record holds; `clock` is now, in UTC.
    pub fn walk(
        stack: &mut UndoStack,
        span: &[EntryId],
        host: &dyn UndoHost,
        measure: &dyn Fn(&dyn UndoableRecord) -> HeldSize,
        clock: &dyn Fn() -> FileTime,
    ) -> NavigationResult {
        let mut refreshes = Vec::new();
        let mut released = Vec::new();
        let mut notes = Vec::new();
        let mut steps = 0;

        for &id in span {
            let result = stack
                .entry(id)
                .expect("a span names only entries in the history")
                .record()
                .step(host);
            refreshes.push(result.refresh);
            if !result.report.note.is_empty() {
                notes.push(result.report.note.clone());
            }

            if result.report.nothing_happened() {
                stack
                    .refused(id, result.report.clone(), clock())
                    .expect("a span runs beside the cursor");
                return NavigationResult {
                    steps_taken: steps,
                    stopped_at: Some(id),
                    stop_report: Some(result.report),
                    refreshes,
                    released: Released { entries: released },
                    notes,
                };
            }

            let held = measure(result.next.as_ref());
            let clean = result.report.clean();
            let report = result.report;
            released.extend(
                stack
                    .applied(id, result.next, held, report.clone(), clock())
                    .expect("a span runs beside the cursor")
                    .entries,
            );
            steps += 1;

            if !clean {
                return NavigationResult {
                    steps_taken: steps,
                    stopped_at: Some(id),
                    stop_report: Some(report),
                    refreshes,
                    released: Released { entries: released },
                    notes,
                };
            }
        }

        NavigationResult {
            steps_taken: steps,
            stopped_at: None,
            stop_report: None,
            refreshes,
            released: Released { entries: released },
            notes,
        }
    }
}
