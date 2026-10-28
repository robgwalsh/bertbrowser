// Port of tests/BertBrowser.Core.Tests/RedoPlanTests.cs
//
// The plans a redo runs, built from what an undo put back. Pure: the rules here are what keep a
// redo from displacing anything nobody chose to replace.
use bb_core::delete::{DeleteDisposition, DeleteMode, DeleteRedo, DeletedItem};
use bb_core::transfer::{CompletedTransfer, ConflictResolution, TransferRedo, TransferVerb};
use bb_paths::PathKey;

fn item(name: &str, displaced: bool) -> CompletedTransfer {
    CompletedTransfer {
        source_path: format!(r"C:\src\{name}"),
        final_path: format!(r"C:\dst\{name}"),
        is_directory: false,
        displaced_stage_path: displaced.then(|| format!(r"C:\dst\.bertbrowser-replaced-1\{name}")),
        written: None,
    }
}

fn deleted(source: &str, staged: Option<&str>, recycled: Option<&str>) -> DeletedItem {
    DeletedItem {
        source_path: source.to_string(),
        is_directory: false,
        staged_path: staged.map(str::to_string),
        recycled_path: recycled.map(str::to_string),
    }
}

#[test]
fn each_item_lands_where_it_landed_before() {
    let (plan, _) = TransferRedo::plan_for(
        TransferVerb::Move,
        r"C:\dst",
        &[item("a (2).txt", false)],
        &[],
    );

    assert_eq!(1, plan.transfers.len());
    assert_eq!(r"C:\dst\a (2).txt", plan.transfers[0].destination_path);
}

#[test]
fn only_what_was_displaced_is_displaced_again_as_a_replace_for_a_move() {
    let (_, resolutions) = TransferRedo::plan_for(
        TransferVerb::Move,
        r"C:\dst",
        &[item("a.txt", true), item("b.txt", false)],
        &[],
    );

    assert_eq!(
        Some(&ConflictResolution::Replace),
        resolutions.get(&PathKey::canonicalize(r"C:\src\a.txt").unwrap())
    );
    assert_eq!(
        Some(&ConflictResolution::Skip),
        resolutions.get(&PathKey::canonicalize(r"C:\src\b.txt").unwrap())
    );
}

#[test]
fn a_copy_displaces_with_overwrite_since_a_copy_never_replaces() {
    let (_, resolutions) =
        TransferRedo::plan_for(TransferVerb::Copy, r"C:\dst", &[item("a.txt", true)], &[]);

    assert_eq!(1, resolutions.len());
    assert_eq!(
        Some(&ConflictResolution::Overwrite),
        resolutions.values().next()
    );
}

#[test]
fn the_original_order_comes_back_so_a_folder_is_written_before_its_contents() {
    // An undo walks newest first, so that is the order its list arrives in.
    let (plan, _) = TransferRedo::plan_for(
        TransferVerb::Copy,
        r"C:\dst",
        &[item(r"dir\a.txt", false), item("dir", false)],
        &[],
    );

    let sources: Vec<&str> = plan
        .transfers
        .iter()
        .map(|t| t.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\dir", r"C:\src\dir\a.txt"], sources);
}

#[test]
fn only_a_move_prunes() {
    let pruned = vec![r"C:\src\Photos".to_string()];

    assert_eq!(
        pruned,
        TransferRedo::plan_for(
            TransferVerb::Move,
            r"C:\dst",
            &[item("a.txt", false)],
            &pruned
        )
        .0
        .prune_directories
    );
    assert!(
        TransferRedo::plan_for(
            TransferVerb::Copy,
            r"C:\dst",
            &[item("a.txt", false)],
            &pruned
        )
        .0
        .prune_directories
        .is_empty()
    );
}

#[test]
fn a_delete_goes_back_the_way_it_went() {
    let plan = DeleteRedo::plan_for(&[
        deleted(
            r"C:\w\held.txt",
            Some(r"C:\.bertbrowser-trash\delete-1\held.txt"),
            None,
        ),
        deleted(
            r"C:\w\binned.txt",
            None,
            Some(r"C:\$Recycle.Bin\S-1\$R1.txt"),
        ),
    ]);

    assert_eq!(DeleteMode::Recycle, plan.mode);
    assert!(!plan.permanent());
    let binned = plan
        .deletions
        .iter()
        .find(|d| d.name() == "binned.txt")
        .unwrap();
    assert_eq!(DeleteDisposition::Recycle, binned.disposition);
    let held = plan
        .deletions
        .iter()
        .find(|d| d.name() == "held.txt")
        .unwrap();
    assert_eq!(DeleteDisposition::Stage, held.disposition);
}

#[test]
fn a_delete_that_was_only_held_is_redone_as_held() {
    let plan = DeleteRedo::plan_for(&[deleted(r"C:\w\a.txt", Some(r"C:\t\a.txt"), None)]);

    assert_eq!(DeleteMode::Staged, plan.mode);
}
