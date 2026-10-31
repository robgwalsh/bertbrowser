// Port of tests/BertBrowser.Core.Tests/UndoNavigatorTests.cs
//
// "Undo to here" and "redo to here": a walk of single steps that stops at the first one that did
// not go cleanly, so nothing runs on top of an operation that is only half undone.
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::delete::{DeleteExecutor, FileSystemDeleteProbe};
use bb_core::rename::RenameExecutor;
use bb_core::transfer::TransferExecutor;
use bb_core::undo::{
    EntryId, HeldSize, NavigationResult, PlainUndoHost, StepReport, StepVerdict, UndoBudget,
    UndoNavigator, UndoStack,
};

use crate::undo::undo_stack_tests::{FakeHandle, FakeRecord};

fn now() -> FileTime {
    FileTime::from_ymd_hms(2026, 9, 25, 12, 0, 0)
}

struct Fixture {
    stack: UndoStack,
    transfers: TransferExecutor,
    deletes: DeleteExecutor<'static>,
    renames: RenameExecutor,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            stack: UndoStack::new(UndoBudget::default()),
            transfers: TransferExecutor::default(),
            deletes: DeleteExecutor::new(&FileSystemDeleteProbe),
            renames: RenameExecutor,
        }
    }

    fn push(&mut self, name: &str) -> (EntryId, Arc<FakeHandle>) {
        let record = FakeRecord::new(name);
        let handle = record.handle();
        let (id, _) = self
            .stack
            .push(Box::new(record), HeldSize::NONE, now())
            .unwrap();
        (id, handle)
    }

    /// The fakes never touch the host, so a plain one over idle executors stands in for it.
    fn walk(&mut self, span: &[EntryId]) -> NavigationResult {
        let host = PlainUndoHost::without_archives(&self.transfers, &self.deletes, &self.renames);
        UndoNavigator::walk(&mut self.stack, span, &host, &|_| HeldSize::NONE, &now)
    }

    fn is_done(&self, id: EntryId) -> bool {
        self.stack.entry(id).unwrap().is_done()
    }
}

#[test]
fn undoing_to_an_entry_undoes_it_and_everything_after_it() {
    let mut f = Fixture::new();
    let (a, _) = f.push("a");
    let (b, _) = f.push("b");
    let (c, _) = f.push("c");

    let span = f.stack.undo_span(a).unwrap();
    let result = f.walk(&span);

    assert_eq!(3, result.steps_taken);
    assert!(result.stopped_at.is_none());
    for id in [a, b, c] {
        assert!(!f.is_done(id));
    }
    assert_eq!(Some(a), f.stack.next_redo().map(|e| e.id()));
    assert_eq!(3, result.refreshes.len());
}

#[test]
fn redoing_to_an_entry_redoes_up_to_it_and_no_further() {
    let mut f = Fixture::new();
    let (a, _) = f.push("a");
    let (b, _) = f.push("b");
    let (c, _) = f.push("c");
    let span = f.stack.undo_span(a).unwrap();
    f.walk(&span);

    let span = f.stack.redo_span(b).unwrap();
    let result = f.walk(&span);

    assert_eq!(2, result.steps_taken);
    assert!(f.is_done(a));
    assert!(f.is_done(b));
    assert!(!f.is_done(c));
}

/// A step that half worked is taken and reported, and the walk goes no further.
#[test]
fn a_partial_step_is_the_last_one() {
    let mut f = Fixture::new();
    let (a, a_handle) = f.push("a");
    let (b, b_handle) = f.push("b");
    let (c, _) = f.push("c");
    b_handle.set_report(StepReport::new(
        1,
        vec!["one could not be put back".to_string()],
    ));

    let span = f.stack.undo_span(a).unwrap();
    let result = f.walk(&span);

    assert_eq!(2, result.steps_taken);
    assert_eq!(Some(b), result.stopped_at);
    assert!(!f.is_done(c));
    assert!(!f.is_done(b));
    assert!(f.is_done(a));
    assert_eq!(0, a_handle.steps());
}

/// A step that moved nothing leaves its entry where it was, and stops the walk.
#[test]
fn a_failed_step_leaves_its_entry_and_stops() {
    let mut f = Fixture::new();
    let (a, _) = f.push("a");
    let (b, b_handle) = f.push("b");
    b_handle.set_report(StepReport::new(
        0,
        vec!["everything was in the way".to_string()],
    ));

    let span = f.stack.undo_span(a).unwrap();
    let result = f.walk(&span);

    assert_eq!(0, result.steps_taken);
    assert_eq!(Some(b), result.stopped_at);
    assert!(f.is_done(b));
    assert_eq!(Some(b), f.stack.next_undo().map(|e| e.id()));
    assert_eq!(
        StepVerdict::Failed,
        f.stack.entry(b).unwrap().last_report().unwrap().verdict()
    );
}
