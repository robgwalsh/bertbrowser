// Port of tests/BertBrowser.Core.Tests/TransferEstimatorTests.cs. What a plan costs in bytes,
// before it runs. Plans are built by hand rather than through the planner: nothing here touches
// disk, and the point is the arithmetic and the two rules around it: a rename costs nothing, and a
// size the index does not have is unknown rather than zero.
use std::collections::HashMap;

use bb_core::transfer::{
    DirectorySize, PlannedTransfer, TransferEstimator, TransferPlan, TransferSizeSource,
    TransferVerb,
};
use bb_paths::fold_upper;

#[derive(Default)]
struct FakeSizes {
    directories: HashMap<String, DirectorySize>,
    files: HashMap<String, i64>,
}

impl FakeSizes {
    fn set_directory(&mut self, path: &str, bytes: i64, files: usize) {
        self.directories
            .insert(fold_upper(path), DirectorySize { bytes, files });
    }

    fn set_file(&mut self, path: &str, bytes: i64) {
        self.files.insert(fold_upper(path), bytes);
    }
}

impl TransferSizeSource for FakeSizes {
    fn directory(&self, path: &str) -> Option<DirectorySize> {
        self.directories.get(&fold_upper(path)).copied()
    }

    fn file(&self, path: &str) -> Option<i64> {
        self.files.get(&fold_upper(path)).copied()
    }
}

fn plan(verb: TransferVerb, transfers: Vec<PlannedTransfer>) -> TransferPlan {
    TransferPlan::new(verb, r"D:\dest", transfers, Vec::new())
}

fn file(source: &str, destination: &str) -> PlannedTransfer {
    PlannedTransfer::new(source, false, destination, false)
}

fn folder(source: &str, destination: &str) -> PlannedTransfer {
    PlannedTransfer::new(source, true, destination, false)
}

#[test]
fn adds_up_the_files_and_folders_it_is_given() {
    let mut sizes = FakeSizes::default();
    sizes.set_file(r"C:\src\a.txt", 1_000);
    sizes.set_directory(r"C:\src\tree", 9_000, 4);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Copy,
            vec![
                file(r"C:\src\a.txt", r"D:\dest\a.txt"),
                folder(r"C:\src\tree", r"D:\dest\tree"),
            ],
        ),
        &sizes,
    );

    assert_eq!(10_000, estimate.bytes);
    assert_eq!(5, estimate.files);
    assert!(estimate.complete);
}

#[test]
fn a_same_volume_move_costs_nothing_because_it_is_a_rename() {
    // The load-bearing rule. Counting these bytes puts a bar on screen that sits at zero while
    // instant renames go past, then jumps to done.
    let mut sizes = FakeSizes::default();
    sizes.set_directory(r"C:\src\huge", 50 * 1024 * 1024 * 1024, 100_000);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Move,
            vec![folder(r"C:\src\huge", r"C:\dest\huge")],
        ),
        &sizes,
    );

    assert_eq!(0, estimate.bytes);
    assert_eq!(0, estimate.files);
    assert!(estimate.complete);
}

#[test]
fn a_cross_volume_move_costs_its_full_size() {
    let mut sizes = FakeSizes::default();
    sizes.set_directory(r"C:\src\tree", 4_096, 2);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Move,
            vec![folder(r"C:\src\tree", r"D:\dest\tree")],
        ),
        &sizes,
    );

    assert_eq!(4_096, estimate.bytes);
}

#[test]
fn a_copy_within_one_volume_still_costs_its_full_size() {
    let mut sizes = FakeSizes::default();
    sizes.set_file(r"C:\src\a.txt", 512);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Copy,
            vec![file(r"C:\src\a.txt", r"C:\dest\a.txt")],
        ),
        &sizes,
    );

    assert_eq!(512, estimate.bytes);
}

#[test]
fn a_folder_the_index_has_no_row_for_makes_the_total_a_floor() {
    // A non-NTFS volume, or one still being indexed. The bytes we do know are still worth having;
    // what must not happen is presenting them as the total.
    let mut sizes = FakeSizes::default();
    sizes.set_file(r"C:\src\a.txt", 1_000);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Copy,
            vec![
                file(r"C:\src\a.txt", r"D:\dest\a.txt"),
                folder(r"C:\src\tree", r"D:\dest\tree"),
            ],
        ),
        &sizes,
    );

    assert_eq!(1_000, estimate.bytes);
    assert!(!estimate.complete);
    assert!(!estimate.is_usable());
}

#[test]
fn an_unknown_size_is_not_treated_as_zero() {
    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Copy,
            vec![folder(r"C:\src\tree", r"D:\dest\tree")],
        ),
        &FakeSizes::default(),
    );

    assert!(!estimate.complete);
    assert!(!estimate.is_usable());
}

#[test]
fn an_empty_plan_is_complete_and_costs_nothing() {
    let estimate =
        TransferEstimator::estimate(&plan(TransferVerb::Copy, vec![]), &FakeSizes::default());

    assert_eq!(0, estimate.bytes);
    assert!(estimate.complete);
    assert!(!estimate.is_usable()); // nothing to draw a bar against
}

#[test]
fn a_plan_of_nothing_but_renames_is_complete_but_not_usable() {
    let mut sizes = FakeSizes::default();
    sizes.set_file(r"C:\src\a.txt", 1_000);

    let estimate = TransferEstimator::estimate(
        &plan(
            TransferVerb::Move,
            vec![file(r"C:\src\a.txt", r"C:\dest\a.txt")],
        ),
        &sizes,
    );

    assert!(estimate.complete);
    assert!(!estimate.is_usable());
}
