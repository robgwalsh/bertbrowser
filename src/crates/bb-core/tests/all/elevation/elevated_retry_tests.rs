// Port of tests/BertBrowser.Core.Tests/ElevatedRetryTests.cs
//
// Which failures are worth a UAC prompt, what the second pass is asked to do, and how the two
// results become one. Pure: record literals in, record literals out, no disk and no prompt.
//
// The merge tests are the ones to keep honest. A merged outcome has to be indistinguishable from
// one the executor produced alone, because the undo slot, the staging commit and the tab fan-out
// all read it without knowing elevation happened: so an outcome that loses a staging folder loses
// the user's displaced data for good, and one that keeps a failure the retry fixed reports a
// problem that no longer exists.
use bb_core::delete::{
    DeleteMode, DeleteOutcome, DeletePlan, DeleteUndoResult, DeletedItem, FailedDelete,
    PlannedDelete,
};
use bb_core::elevation::{ElevatedRetry, ElevationRules};
use bb_core::newitem::{FailedNewItem, NewItemKind, NewItemOutcome, NewItemPlan};
use bb_core::rename::{CompletedRename, FailedRename, PlannedRename, RenameOutcome, RenamePlan};
use bb_core::transfer::{
    CompletedTransfer, ConflictResolution, FailedTransfer, PlannedTransfer, RejectedTransfer,
    Resolutions, TransferOutcome, TransferPlan, TransferRejection, TransferUndoResult,
    TransferVerb,
};
use bb_paths::{PathKey, file_name};

const DEST: &str = r"C:\dest";

// --- helpers ---

fn planned(source: &str) -> PlannedTransfer {
    PlannedTransfer::new(
        source,
        false,
        &format!("{DEST}\\{}", file_name(source)),
        false,
    )
}

fn completed(source: &str) -> CompletedTransfer {
    CompletedTransfer {
        source_path: source.to_string(),
        final_path: format!("{DEST}\\{}", file_name(source)),
        is_directory: false,
        displaced_stage_path: None,
        written: None,
    }
}

fn denied(source: &str) -> FailedTransfer {
    FailedTransfer {
        source_path: source.to_string(),
        message: "Access to the path is denied.".to_string(),
        access_denied: true,
    }
}

fn in_use(source: &str, message: &str) -> FailedTransfer {
    FailedTransfer::new(source, message.to_string())
}

fn move_plan(sources: &[&str]) -> TransferPlan {
    TransferPlan::new(
        TransferVerb::Move,
        DEST,
        sources.iter().map(|s| planned(s)).collect(),
        Vec::new(),
    )
}

fn move_outcome(
    completed: Vec<CompletedTransfer>,
    failed: Vec<FailedTransfer>,
    staging: Vec<&str>,
    cancelled: bool,
) -> TransferOutcome {
    TransferOutcome::new(
        TransferVerb::Move,
        DEST,
        completed,
        Vec::new(),
        failed,
        staging.into_iter().map(str::to_string).collect(),
        cancelled,
    )
}

fn denied_delete(source: &str) -> FailedDelete {
    FailedDelete {
        source_path: source.to_string(),
        message: "denied".to_string(),
        access_denied: true,
    }
}

fn recycled(source: &str, recycled: &str) -> DeletedItem {
    DeletedItem {
        source_path: source.to_string(),
        is_directory: false,
        staged_path: None,
        recycled_path: Some(recycled.to_string()),
    }
}

fn new_item_plan() -> NewItemPlan {
    NewItemPlan {
        directory: r"C:\Program Files\Vendor".to_string(),
        name: "notes.txt".to_string(),
        kind: NewItemKind::File,
        template_path: None,
        rejected: None,
    }
}

fn stranded_rename(stranded: &str) -> RenameOutcome {
    RenameOutcome {
        completed: Vec::new(),
        failed: vec![FailedRename {
            source_path: r"C:\src\a.txt".to_string(),
            message: "denied".to_string(),
            access_denied: true,
            stranded_path: Some(stranded.to_string()),
        }],
    }
}

// --- is there anything a token could fix? ---

#[test]
fn an_outcome_with_only_ordinary_failures_is_not_retryable() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let outcome = move_outcome(
        Vec::new(),
        vec![in_use(r"C:\src\a.txt", "in use")],
        Vec::new(),
        false,
    );

    assert!(ElevatedRetry::retry_for_transfer(&plan, &outcome, None).is_none());
}

#[test]
fn an_outcome_with_nothing_wrong_is_not_retryable() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let outcome = move_outcome(Vec::new(), Vec::new(), Vec::new(), false);

    assert!(ElevatedRetry::retry_for_transfer(&plan, &outcome, None).is_none());
}

#[test]
fn a_cancelled_run_is_not_retryable_even_when_something_was_denied() {
    // A cancel and a denial can coexist: item one refused, cancel pressed at item five. Putting a
    // consent prompt in front of somebody who has just pressed Cancel is the wrong answer whatever
    // else is true.
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let outcome = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), true);

    assert!(ElevatedRetry::retry_for_transfer(&plan, &outcome, None).is_none());
}

#[test]
fn elevation_is_never_offered_for_a_protected_location() {
    for variable in ["SystemRoot", "ProgramFiles", "USERPROFILE"] {
        let path =
            std::env::var(variable).unwrap_or_else(|_| panic!("{variable} is set on Windows"));
        assert!(
            ElevationRules::is_refused_for_elevation(&path),
            "{variable} = {path}"
        );

        let plan = move_plan(&[&path]);
        let outcome = move_outcome(Vec::new(), vec![denied(&path)], Vec::new(), false);

        assert!(
            ElevatedRetry::retry_for_transfer(&plan, &outcome, None).is_none(),
            "{variable} = {path}"
        );
    }
}

#[test]
fn elevation_is_never_offered_for_something_inside_the_recycle_bin() {
    assert!(ElevationRules::is_refused_for_elevation(
        r"C:\$Recycle.Bin\S-1-5-21-1\$RAB1234.txt"
    ));
}

#[test]
fn an_ordinary_folder_is_not_refused() {
    assert!(!ElevationRules::is_refused_for_elevation(
        r"C:\Program Files\Vendor\app.exe"
    ));
}

// --- what the second pass is asked to do ---

#[test]
fn the_retry_carries_only_the_denied_items() {
    let plan = move_plan(&[r"C:\src\a.txt", r"C:\src\b.txt", r"C:\src\c.txt"]);
    let outcome = move_outcome(
        Vec::new(),
        vec![denied(r"C:\src\a.txt"), in_use(r"C:\src\b.txt", "in use")],
        Vec::new(),
        false,
    );

    let retry = ElevatedRetry::retry_for_transfer(&plan, &outcome, None).unwrap();

    let sources: Vec<&str> = retry
        .plan
        .transfers
        .iter()
        .map(|t| t.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\a.txt"], sources);
}

#[test]
fn the_retry_matches_items_by_key_rather_than_by_string() {
    // The plan and the failure disagree about casing, which is what PathKey exists to settle.
    let plan = move_plan(&[r"C:\src\A.txt"]);
    let outcome = move_outcome(Vec::new(), vec![denied(r"c:\SRC\a.TXT")], Vec::new(), false);

    let retry = ElevatedRetry::retry_for_transfer(&plan, &outcome, None).unwrap();

    assert_eq!(1, retry.plan.transfers.len());
}

#[test]
fn the_retry_carries_no_rejections() {
    let plan = TransferPlan::new(
        TransferVerb::Move,
        DEST,
        vec![planned(r"C:\src\a.txt")],
        vec![RejectedTransfer::new(
            r"C:\src\z.txt",
            TransferRejection::MovesWithAncestor,
            "travels with its folder".to_string(),
        )],
    );
    let outcome = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);

    let retry = ElevatedRetry::retry_for_transfer(&plan, &outcome, None).unwrap();

    assert!(retry.plan.rejected.is_empty());
}

#[test]
fn the_retry_keeps_the_conflict_resolution_the_user_chose() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let outcome = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);
    let mut chosen = Resolutions::new();
    chosen.insert(
        PathKey::canonicalize(r"C:\src\a.txt").unwrap(),
        ConflictResolution::Replace,
    );

    let retry = ElevatedRetry::retry_for_transfer(&plan, &outcome, Some(&chosen)).unwrap();

    assert_eq!(1, retry.resolutions.len());
    assert_eq!(
        Some(&ConflictResolution::Replace),
        retry.resolutions.values().next()
    );
}

#[test]
fn a_delete_retry_keeps_the_mode_and_drops_the_items_that_succeeded() {
    let plan = DeletePlan {
        mode: DeleteMode::Recycle,
        deletions: vec![
            PlannedDelete::new(r"C:\src\a.txt", false),
            PlannedDelete::new(r"C:\src\b.txt", false),
        ],
        rejected: Vec::new(),
    };
    let outcome = DeleteOutcome {
        permanent: false,
        deleted: Vec::new(),
        failed: vec![denied_delete(r"C:\src\b.txt")],
        staging_directories: Vec::new(),
    };

    let retry = ElevatedRetry::retry_for_delete(&plan, &outcome).unwrap();

    assert_eq!(DeleteMode::Recycle, retry.plan.mode);
    let sources: Vec<&str> = retry
        .plan
        .deletions
        .iter()
        .map(|d| d.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\b.txt"], sources);
}

#[test]
fn a_stranded_rename_is_retried_from_where_the_item_actually_is() {
    // The forward rename staged the item out from under itself, then failed, then could not put
    // it back. Retrying from the path the plan names would rename from somewhere that no longer
    // holds anything, and report success.
    let stranded = r"C:\src\.bertbrowser-rename-abc";
    let plan = RenamePlan::new(
        vec![PlannedRename::new(r"C:\src\a.txt", r"C:\src\b.txt", false)],
        Vec::new(),
    );
    let outcome = stranded_rename(stranded);

    let retry = ElevatedRetry::retry_for_rename(&plan, &outcome).unwrap();

    assert_eq!(1, retry.plan.renames.len());
    assert_eq!(stranded, retry.plan.renames[0].source_path);
    assert_eq!(r"C:\src\b.txt", retry.plan.renames[0].target_path);
}

#[test]
fn creating_retries_the_whole_plan_because_there_is_only_ever_one_item() {
    let plan = new_item_plan();
    let outcome = NewItemOutcome {
        created_path: None,
        failed: Some(FailedNewItem {
            message: "denied".to_string(),
            access_denied: true,
        }),
    };

    let retry = ElevatedRetry::retry_for_new_item(&plan, &outcome).unwrap();

    assert_eq!(plan, retry.plan);
}

#[test]
fn creating_is_not_retried_when_the_name_was_simply_taken() {
    let plan = new_item_plan();
    let outcome = NewItemOutcome {
        created_path: None,
        failed: Some(FailedNewItem::new(
            "'notes.txt' already exists in this folder.".to_string(),
        )),
    };

    assert!(ElevatedRetry::retry_for_new_item(&plan, &outcome).is_none());
}

// --- folding the two results into one ---

#[test]
fn merging_drops_the_failure_the_retry_fixed() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let first = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );

    let merged = ElevatedRetry::merge_transfer(&first, &retry, &second).unwrap();

    assert!(merged.failed.is_empty());
    assert_eq!(1, merged.completed.len());
}

#[test]
fn merging_keeps_a_failure_the_retry_could_not_fix() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let first = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);

    let merged = ElevatedRetry::merge_transfer(&first, &retry, &second).unwrap();

    assert_eq!(1, merged.failed.len());
}

#[test]
fn merging_leaves_a_failure_nobody_retried_alone() {
    let plan = move_plan(&[r"C:\src\a.txt", r"C:\src\b.txt"]);
    let first = move_outcome(
        Vec::new(),
        vec![denied(r"C:\src\a.txt"), in_use(r"C:\src\b.txt", "in use")],
        Vec::new(),
        false,
    );
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );

    let merged = ElevatedRetry::merge_transfer(&first, &retry, &second).unwrap();

    let failed: Vec<&str> = merged
        .failed
        .iter()
        .map(|f| f.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\b.txt"], failed);
}

#[test]
fn merging_keeps_both_staging_folders() {
    // The one whose real-world failure is data loss: a staging folder dropped here is never
    // committed and never purged, so what a Replace displaced stays hidden on disk for good.
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let first = move_outcome(
        Vec::new(),
        vec![denied(r"C:\src\a.txt")],
        vec![r"C:\dest\.bertbrowser-replaced-1"],
        false,
    );
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        vec![r"C:\dest\.bertbrowser-replaced-2"],
        false,
    );

    let merged = ElevatedRetry::merge_transfer(&first, &retry, &second).unwrap();

    assert_eq!(2, merged.staging_directories.len());
}

#[test]
fn a_merged_outcome_is_still_undoable() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let first = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );

    assert!(
        ElevatedRetry::merge_transfer(&first, &retry, &second)
            .unwrap()
            .can_undo()
    );
}

#[test]
fn merging_refuses_two_passes_that_disagree_about_the_operation() {
    let plan = move_plan(&[r"C:\src\a.txt"]);
    let first = move_outcome(Vec::new(), vec![denied(r"C:\src\a.txt")], Vec::new(), false);
    let retry = ElevatedRetry::retry_for_transfer(&plan, &first, None).unwrap();
    let second = TransferOutcome::new(
        TransferVerb::Copy,
        DEST,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        false,
    );

    assert!(ElevatedRetry::merge_transfer(&first, &retry, &second).is_err());
}

#[test]
fn a_merged_rename_reports_the_original_source_of_a_stranded_item() {
    // Undo is this same execution with every path swapped, so a completed record naming the
    // staging path would put the item back under the staging name rather than where it came from.
    let stranded = r"C:\src\.bertbrowser-rename-abc";
    let plan = RenamePlan::new(
        vec![PlannedRename::new(r"C:\src\a.txt", r"C:\src\b.txt", false)],
        Vec::new(),
    );
    let first = stranded_rename(stranded);
    let retry = ElevatedRetry::retry_for_rename(&plan, &first).unwrap();
    let second = RenameOutcome {
        completed: vec![CompletedRename {
            source_path: stranded.to_string(),
            final_path: r"C:\src\b.txt".to_string(),
            is_directory: false,
        }],
        failed: Vec::new(),
    };

    let merged = ElevatedRetry::merge_rename(&first, &retry, &second);

    assert_eq!(1, merged.completed.len());
    assert_eq!(r"C:\src\a.txt", merged.completed[0].source_path);
    assert!(merged.failed.is_empty());
}

#[test]
fn merging_deletes_keeps_both_holding_folders() {
    let plan = DeletePlan {
        mode: DeleteMode::Staged,
        deletions: vec![PlannedDelete::new(r"C:\src\a.txt", false)],
        rejected: Vec::new(),
    };
    let first = DeleteOutcome {
        permanent: false,
        deleted: Vec::new(),
        failed: vec![denied_delete(r"C:\src\a.txt")],
        staging_directories: vec![r"C:\.bertbrowser-trash\delete-1".to_string()],
    };
    let retry = ElevatedRetry::retry_for_delete(&plan, &first).unwrap();
    let second = DeleteOutcome {
        permanent: false,
        deleted: vec![DeletedItem {
            source_path: r"C:\src\a.txt".to_string(),
            is_directory: false,
            staged_path: Some(r"C:\.bertbrowser-trash\delete-2\a.txt".to_string()),
            recycled_path: None,
        }],
        failed: Vec::new(),
        staging_directories: vec![r"C:\.bertbrowser-trash\delete-2".to_string()],
    };

    let merged = ElevatedRetry::merge_delete(&first, &retry, &second).unwrap();

    assert_eq!(2, merged.staging_directories.len());
    assert!(merged.failed.is_empty());
    assert!(merged.can_undo());
}

// --- putting things back ---

#[test]
fn an_undo_retry_carries_only_the_items_that_could_not_be_put_back() {
    let outcome = move_outcome(
        vec![completed(r"C:\src\a.txt"), completed(r"C:\src\b.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );
    let result = TransferUndoResult {
        restored: 1,
        failed: vec![denied(r"C:\src\b.txt")],
        reverted: Vec::new(),
    };

    let retry = ElevatedRetry::undo_retry_for_transfer(&outcome, &result).unwrap();

    let sources: Vec<&str> = retry
        .outcome
        .completed
        .iter()
        .map(|c| c.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\b.txt"], sources);
}

#[test]
fn an_undo_retry_carries_no_staging_folders() {
    // The unelevated half still holds items in its own, and the elevated pass's cleanup has no
    // business running over them.
    let outcome = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        vec![r"C:\dest\.bertbrowser-replaced-1"],
        false,
    );
    let result = TransferUndoResult {
        restored: 0,
        failed: vec![denied(r"C:\src\a.txt")],
        reverted: Vec::new(),
    };

    let retry = ElevatedRetry::undo_retry_for_transfer(&outcome, &result).unwrap();

    assert!(retry.outcome.staging_directories.is_empty());
}

#[test]
fn an_undo_that_failed_for_some_other_reason_is_not_retried() {
    let outcome = move_outcome(
        vec![completed(r"C:\src\a.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );
    let result = TransferUndoResult {
        restored: 0,
        failed: vec![in_use(r"C:\src\a.txt", "it is no longer there")],
        reverted: Vec::new(),
    };

    assert!(ElevatedRetry::undo_retry_for_transfer(&outcome, &result).is_none());
}

#[test]
fn merging_an_undo_adds_the_counts_and_drops_what_the_retry_fixed() {
    let outcome = move_outcome(
        vec![completed(r"C:\src\a.txt"), completed(r"C:\src\b.txt")],
        Vec::new(),
        Vec::new(),
        false,
    );
    let first = TransferUndoResult {
        restored: 1,
        failed: vec![denied(r"C:\src\b.txt")],
        reverted: Vec::new(),
    };
    let retry = ElevatedRetry::undo_retry_for_transfer(&outcome, &first).unwrap();
    let second = TransferUndoResult {
        restored: 1,
        failed: Vec::new(),
        reverted: Vec::new(),
    };

    let merged = ElevatedRetry::merge_transfer_undo(&first, &retry, &second);

    assert_eq!(2, merged.restored);
    assert!(merged.failed.is_empty());
}

#[test]
fn merging_a_delete_undo_works_the_same_way() {
    let outcome = DeleteOutcome {
        permanent: false,
        deleted: vec![recycled(
            r"C:\src\a.txt",
            r"C:\$Recycle.Bin\S-1-5-21-1\$RAA.txt",
        )],
        failed: Vec::new(),
        staging_directories: Vec::new(),
    };
    let first = DeleteUndoResult {
        restored: 0,
        failed: vec![denied_delete(r"C:\src\a.txt")],
        reverted: Vec::new(),
    };
    let retry = ElevatedRetry::undo_retry_for_delete(&outcome, &first).unwrap();
    let second = DeleteUndoResult {
        restored: 1,
        failed: Vec::new(),
        reverted: Vec::new(),
    };

    let merged = ElevatedRetry::merge_delete_undo(&first, &retry, &second);

    assert_eq!(1, merged.restored);
    assert!(merged.failed.is_empty());
}

/// The pipe reports a count, never a list, so the elevated half of what went back is worked out
/// here, or a redo would skip everything the retry restored.
#[test]
fn merging_an_undo_works_out_what_the_elevated_half_put_back() {
    let outcome = move_outcome(
        vec![
            completed(r"C:\src\a.txt"),
            completed(r"C:\src\b.txt"),
            completed(r"C:\src\c.txt"),
        ],
        Vec::new(),
        Vec::new(),
        false,
    );
    let first = TransferUndoResult {
        restored: 1,
        failed: vec![denied(r"C:\src\b.txt"), denied(r"C:\src\c.txt")],
        reverted: vec![completed(r"C:\src\a.txt")],
    };
    let retry = ElevatedRetry::undo_retry_for_transfer(&outcome, &first).unwrap();
    let second = TransferUndoResult {
        restored: 1,
        failed: vec![in_use(r"C:\src\c.txt", "still refused")],
        reverted: Vec::new(),
    };

    let merged = ElevatedRetry::merge_transfer_undo(&first, &retry, &second);

    let reverted: Vec<&str> = merged
        .reverted
        .iter()
        .map(|r| r.source_path.as_str())
        .collect();
    assert_eq!(vec![r"C:\src\a.txt", r"C:\src\b.txt"], reverted);
}

#[test]
fn merging_a_delete_undo_works_out_what_the_elevated_half_put_back() {
    let item = recycled(r"C:\src\a.txt", r"C:\$Recycle.Bin\S-1-5-21-1\$RAA.txt");
    let outcome = DeleteOutcome {
        permanent: false,
        deleted: vec![item.clone()],
        failed: Vec::new(),
        staging_directories: Vec::new(),
    };
    let first = DeleteUndoResult {
        restored: 0,
        failed: vec![denied_delete(r"C:\src\a.txt")],
        reverted: Vec::new(),
    };
    let retry = ElevatedRetry::undo_retry_for_delete(&outcome, &first).unwrap();

    let merged = ElevatedRetry::merge_delete_undo(
        &first,
        &retry,
        &DeleteUndoResult {
            restored: 1,
            failed: Vec::new(),
            reverted: Vec::new(),
        },
    );

    assert_eq!(vec![item], merged.reverted);
}
