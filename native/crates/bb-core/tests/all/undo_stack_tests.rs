// Port of tests/BertBrowser.Core.Tests/UndoStackTests.cs
//
// The history's rules, with records that touch nothing: what a new operation discards, what the
// budget releases and in which order, and what one step may and may not change.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bb_core::FileTime;
use bb_core::undo::{
    EntryId, HeldEntry, HeldSize, Released, StepReport, StepResult, StepVerdict, UndoBudget,
    UndoDirection, UndoHost, UndoItemDetail, UndoKind, UndoRefresh, UndoStack, UndoableRecord,
};

fn now() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 25, 12, 0, 0)
}

fn stack(entries: i32, bytes: i64) -> UndoStack {
    UndoStack::new(UndoBudget::new(entries, bytes))
}

fn push(stack: &mut UndoStack, name: &str, held: i64) -> (EntryId, Released) {
    stack
        .push(
            Box::new(FakeRecord::new(name)),
            HeldSize::new(held, true),
            now(),
        )
        .unwrap()
}

/// Takes the step the stack says is next on `entry`, cleanly.
fn step(stack: &mut UndoStack, entry: EntryId, held: i64) -> Released {
    let flipped = {
        let record = stack.entry(entry).unwrap().record();
        FakeRecord::with_state(record.description(), !record.is_applied())
    };
    stack
        .applied(
            entry,
            Box::new(flipped),
            HeldSize::new(held, true),
            StepReport::new(1, Vec::new()),
            now(),
        )
        .unwrap()
}

fn names(stack: &UndoStack) -> Vec<String> {
    stack
        .entries()
        .iter()
        .map(|e| e.record().description().to_string())
        .collect()
}

fn released_names(released: &Released) -> Vec<String> {
    released
        .entries
        .iter()
        .map(|e| e.record().description().to_string())
        .collect()
}

#[test]
fn a_new_operation_is_done_and_is_the_next_undo() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);

    assert!(stack.entry(a).unwrap().is_done());
    assert_eq!(Some(a), stack.next_undo().map(|e| e.id()));
    assert!(stack.next_redo().is_none());
    assert_eq!(1, stack.cursor());
}

#[test]
fn undoing_moves_the_cursor_and_makes_the_entry_the_next_redo() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);
    let (b, _) = push(&mut stack, "b", 0);

    step(&mut stack, b, 0);

    assert!(!stack.entry(b).unwrap().is_done());
    assert_eq!(Some(a), stack.next_undo().map(|e| e.id()));
    assert_eq!(Some(b), stack.next_redo().map(|e| e.id()));
    assert_eq!(
        Some(UndoDirection::Undo),
        stack.entry(b).unwrap().last_direction()
    );
}

/// One line of history, as in every editor: a new operation after an undo abandons the branch,
/// and hands it back to be retired so what it held is committed.
#[test]
fn a_new_operation_discards_the_redo_branch_and_releases_it() {
    let mut stack = stack(50, 1 << 40);
    push(&mut stack, "a", 0);
    let (b, _) = push(&mut stack, "b", 0);
    let (c, _) = push(&mut stack, "c", 0);
    step(&mut stack, c, 0);
    step(&mut stack, b, 0);

    let (_, released) = push(&mut stack, "d", 0);

    assert_eq!(vec!["a", "d"], names(&stack));
    let mut gone = released_names(&released);
    gone.sort();
    assert_eq!(vec!["b", "c"], gone);
    assert_eq!(0, stack.released_count()); // abandoned, not pushed out by the limits
}

#[test]
fn the_count_cap_releases_the_oldest_first() {
    let mut stack = stack(3, 1 << 40);
    push(&mut stack, "a", 0);
    push(&mut stack, "b", 0);
    push(&mut stack, "c", 0);

    let (_, released) = push(&mut stack, "d", 0);

    assert_eq!(vec!["b", "c", "d"], names(&stack));
    assert_eq!(vec!["a"], released_names(&released));
    assert_eq!(1, stack.released_count());
}

#[test]
fn the_byte_cap_releases_the_oldest_done_entry_first() {
    let mut stack = stack(50, 100);
    push(&mut stack, "a", 40);
    push(&mut stack, "b", 40);

    let (_, released) = push(&mut stack, "c", 40);

    assert_eq!(vec!["b", "c"], names(&stack));
    assert_eq!(40, stack.released_bytes());
    assert_eq!(40, released.held().bytes);
}

/// With nothing old enough left to release, the far end of the redo branch goes next.
#[test]
fn the_byte_cap_then_releases_the_furthest_undone_entry() {
    let mut stack = stack(50, 100);
    let (a, _) = push(&mut stack, "a", 10);
    let (b, _) = push(&mut stack, "b", 10);
    let (c, _) = push(&mut stack, "c", 10);
    step(&mut stack, c, 10);
    step(&mut stack, b, 10);

    // Undoing a grows what it holds (an archive's edited copy, say) past the budget.
    let released = step(&mut stack, a, 85);

    assert_eq!(vec!["a", "b"], names(&stack));
    assert_eq!(vec!["c"], released_names(&released));
}

/// A single operation larger than the whole budget stays undoable: the guarantee the single slot
/// always gave.
#[test]
fn the_next_undo_is_never_released_even_over_budget() {
    let mut stack = stack(1, 10);
    push(&mut stack, "a", 5);

    let (b, released) = push(&mut stack, "b", 1_000);

    assert_eq!(vec!["b"], names(&stack));
    assert_eq!(Some(b), stack.next_undo().map(|e| e.id()));
    assert_eq!(vec!["a"], released_names(&released));
}

#[test]
fn shrinking_the_budget_releases_straight_away() {
    let mut stack = stack(50, 1 << 40);
    push(&mut stack, "a", 0);
    push(&mut stack, "b", 0);
    push(&mut stack, "c", 0);

    let released = stack.set_budget(UndoBudget::new(1, 1 << 40));

    assert_eq!(vec!["c"], names(&stack));
    assert_eq!(2, released.entries.len());
}

/// A size that could only partly be measured counts as what was measured (a floor), never as
/// nothing.
#[test]
fn an_incomplete_size_still_counts_against_the_budget() {
    let mut stack = stack(50, 100);
    stack
        .push(
            Box::new(FakeRecord::new("a")),
            HeldSize::new(80, false),
            now(),
        )
        .unwrap();
    stack
        .push(
            Box::new(FakeRecord::new("b")),
            HeldSize::new(80, false),
            now(),
        )
        .unwrap();

    assert_eq!(vec!["b"], names(&stack));
    assert!(!stack.held().complete);
}

#[test]
fn undo_span_runs_from_the_next_undo_back_to_the_target() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);
    push(&mut stack, "b", 0);
    push(&mut stack, "c", 0);

    let span: Vec<String> = stack
        .undo_span(a)
        .unwrap()
        .iter()
        .map(|id| stack.entry(*id).unwrap().record().description().to_string())
        .collect();
    assert_eq!(vec!["c", "b", "a"], span);
}

#[test]
fn redo_span_runs_from_the_next_redo_forward_to_the_target() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);
    let (b, _) = push(&mut stack, "b", 0);
    let (c, _) = push(&mut stack, "c", 0);
    step(&mut stack, c, 0);
    step(&mut stack, b, 0);
    step(&mut stack, a, 0);

    let span: Vec<String> = stack
        .redo_span(b)
        .unwrap()
        .iter()
        .map(|id| stack.entry(*id).unwrap().record().description().to_string())
        .collect();
    assert_eq!(vec!["a", "b"], span);
}

#[test]
fn a_span_on_the_wrong_side_of_the_cursor_is_refused() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);

    assert!(stack.redo_span(a).is_err());
}

/// Only the entry either side of the cursor can take a step: anything else would undo an
/// operation out from under the ones built on it.
#[test]
fn only_the_entries_beside_the_cursor_can_step() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);
    push(&mut stack, "b", 0);

    let refused = stack.applied(
        a,
        Box::new(FakeRecord::with_state("a", false)),
        HeldSize::NONE,
        StepReport::new(1, Vec::new()),
        now(),
    );
    assert!(refused.is_err());
}

#[test]
fn a_refused_step_leaves_the_entry_where_it_was() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);

    stack
        .refused(a, StepReport::new(0, vec!["nope".to_string()]), now())
        .unwrap();

    let entry = stack.entry(a).unwrap();
    assert!(entry.is_done());
    assert_eq!(1, stack.cursor());
    assert_eq!(StepVerdict::Failed, entry.last_report().unwrap().verdict());
}

#[test]
fn a_partial_step_still_moves_the_cursor() {
    let mut stack = stack(50, 1 << 40);
    let (a, _) = push(&mut stack, "a", 0);

    stack
        .applied(
            a,
            Box::new(FakeRecord::with_state("a", false)),
            HeldSize::NONE,
            StepReport::new(2, vec!["one stuck".to_string()]),
            now(),
        )
        .unwrap();

    let entry = stack.entry(a).unwrap();
    assert!(!entry.is_done());
    assert_eq!(StepVerdict::Partial, entry.last_report().unwrap().verdict());
}

#[test]
fn clearing_releases_everything_without_counting_it_against_the_limits() {
    let mut stack = stack(50, 1 << 40);
    push(&mut stack, "a", 3);
    let (b, _) = push(&mut stack, "b", 4);
    step(&mut stack, b, 0);

    let released = stack.clear();

    assert!(stack.entries().is_empty());
    assert!(stack.next_undo().is_none());
    assert_eq!(2, released.entries.len());
    assert_eq!(0, stack.released_count());
}

#[test]
fn retiring_a_release_retires_every_record_in_it() {
    let mut stack = stack(1, 1 << 40);
    let a = FakeRecord::new("a");
    let a_handle = a.handle();
    stack.push(Box::new(a), HeldSize::NONE, now()).unwrap();
    let (_, released) = push(&mut stack, "b", 0);

    released.retire_all();

    assert!(a_handle.retired());
}

/// What a test can see of a fake record after it has been handed to the stack: how often it was
/// stepped, whether it was retired, and the report its next step will give.
pub struct FakeHandle {
    steps: AtomicUsize,
    retired: AtomicBool,
    report: Mutex<Option<StepReport>>,
}

impl FakeHandle {
    pub fn steps(&self) -> usize {
        self.steps.load(Ordering::SeqCst)
    }

    pub fn retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }

    /// What the next step will report; clean by default.
    pub fn set_report(&self, report: StepReport) {
        *self.report.lock().unwrap() = Some(report);
    }
}

/// A record that touches nothing and does exactly what it is scripted to.
pub struct FakeRecord {
    name: String,
    applied: bool,
    handle: Arc<FakeHandle>,
}

impl FakeRecord {
    pub fn new(name: &str) -> FakeRecord {
        FakeRecord::with_state(name, true)
    }

    pub fn with_state(name: &str, applied: bool) -> FakeRecord {
        FakeRecord {
            name: name.to_string(),
            applied,
            handle: Arc::new(FakeHandle {
                steps: AtomicUsize::new(0),
                retired: AtomicBool::new(false),
                report: Mutex::new(None),
            }),
        }
    }

    pub fn handle(&self) -> Arc<FakeHandle> {
        self.handle.clone()
    }

    /// The other state, sharing this record's handle, so a scripted report follows it.
    fn flipped(&self) -> FakeRecord {
        FakeRecord {
            name: self.name.clone(),
            applied: !self.applied,
            handle: self.handle.clone(),
        }
    }
}

impl UndoableRecord for FakeRecord {
    fn kind(&self) -> UndoKind {
        UndoKind::Rename
    }

    fn is_applied(&self) -> bool {
        self.applied
    }

    fn description(&self) -> &str {
        &self.name
    }

    fn item_count(&self) -> usize {
        1
    }

    fn details(&self) -> Vec<UndoItemDetail> {
        Vec::new()
    }

    fn held(&self) -> Vec<HeldEntry> {
        Vec::new()
    }

    fn step(&self, _host: &dyn UndoHost) -> StepResult {
        self.handle.steps.fetch_add(1, Ordering::SeqCst);
        let report = self
            .handle
            .report
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| StepReport::new(1, Vec::new()));
        let next: Box<dyn UndoableRecord> = if report.nothing_happened() {
            Box::new(FakeRecord {
                name: self.name.clone(),
                applied: self.applied,
                handle: self.handle.clone(),
            })
        } else {
            Box::new(self.flipped())
        };
        StepResult::new(next, report, UndoRefresh::of([Some(self.name.clone())]))
    }

    fn retire(&self) {
        self.handle.retired.store(true, Ordering::SeqCst);
    }
}
