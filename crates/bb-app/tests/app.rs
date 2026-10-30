mod common;

use bb_app::{AppError, AppEvent, SelectOp};
use bb_core::commands::{FocusKind, FocusState, KeyChord};
use bb_core::delete::DeleteMode;
use bb_core::layout::SplitOrientation;
use common::{Fixture, eventually};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ---- navigation and history ----

#[test]
fn the_listing_puts_folders_first_in_natural_order() {
    let f = Fixture::new();
    assert_eq!(
        strings(&[
            "Documents",
            "Pictures",
            "archive.zip",
            "budget.xlsx",
            "notes.txt",
            "photo.jpg",
            "report.md"
        ]),
        f.names()
    );
    let info = f.app.tab_info(f.tab).unwrap();
    assert_eq!("7 item(s)", info.status);
    assert!(!info.is_flattened);
}

#[test]
fn navigate_back_forward_up_and_the_breadcrumb() {
    let f = Fixture::new();
    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.settle();
    assert_eq!(strings(&["nested", "report.md"]), f.names());

    f.app.navigate(f.tab, &f.path("Documents\\nested")).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.can_back && !info.can_forward && info.can_up);
    assert_eq!("nested", info.title);

    f.app.back(f.tab).unwrap();
    f.settle();
    assert_eq!(f.path("Documents"), f.app.tab_info(f.tab).unwrap().path);
    assert!(f.app.tab_info(f.tab).unwrap().can_forward);

    f.app.forward(f.tab).unwrap();
    f.settle();
    assert_eq!(
        f.path("Documents\\nested"),
        f.app.tab_info(f.tab).unwrap().path
    );

    f.app.up(f.tab).unwrap();
    f.settle();
    assert_eq!(f.path("Documents"), f.app.tab_info(f.tab).unwrap().path);

    let crumbs = f.app.breadcrumb(f.tab).unwrap();
    assert_eq!(f.path("Documents"), crumbs.last().unwrap().full_path);
    assert_eq!("Documents", crumbs.last().unwrap().name);
    assert!(crumbs[0].full_path.ends_with('\\'), "{crumbs:?}");
}

#[test]
fn navigating_after_going_back_drops_the_forward_stack() {
    let f = Fixture::new();
    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.app.back(f.tab).unwrap();
    f.settle();
    assert!(f.app.tab_info(f.tab).unwrap().can_forward);
    f.app.navigate(f.tab, &f.path("Pictures")).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(!info.can_forward && info.can_back);
    assert!(f.app.forward(f.tab).is_err());
}

#[test]
fn a_missing_folder_is_refused_and_said() {
    let f = Fixture::new();
    let before = f.app.tab_info(f.tab).unwrap().path;
    assert!(matches!(
        f.app.navigate(f.tab, &f.path("nope")),
        Err(AppError::Invalid(_))
    ));
    let info = f.app.tab_info(f.tab).unwrap();
    assert_eq!(before, info.path);
    assert!(
        info.status.starts_with("Folder not found"),
        "{}",
        info.status
    );
    assert!(!info.can_back);
}

#[test]
fn refresh_reloads_and_up_stops_at_the_root() {
    let f = Fixture::new();
    std::fs::write(f.root.join("late.txt"), b"x").unwrap();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    assert!(f.names().contains(&"late.txt".to_string()));
    f.app.navigate(f.tab, "C:\\").unwrap();
    f.settle();
    assert!(f.app.up(f.tab).is_err());
}

#[test]
fn stale_ids_say_so() {
    let f = Fixture::new();
    let gone = bb_app::TabId(1);
    assert_eq!(Some(AppError::Stale), f.app.navigate(gone, "C:\\").err());
    assert!(matches!(f.app.listing(gone), Err(AppError::Stale)));
    assert!(matches!(f.app.tab_info(gone), Err(AppError::Stale)));
    let extra = f
        .app
        .open_tab(None, &f.root.to_string_lossy(), true)
        .unwrap();
    f.app.close_tab(extra).unwrap();
    assert!(matches!(f.app.tab_info(extra), Err(AppError::Stale)));
}

// ---- sorting ----

#[test]
fn sorting_keeps_folders_in_front_in_both_directions() {
    let f = Fixture::new();
    f.app.set_sort(f.tab, "Size").unwrap();
    // Unknown folder sizes sort first (-1), then files by length.
    assert_eq!(
        strings(&[
            "Documents",
            "Pictures",
            "report.md",
            "notes.txt",
            "budget.xlsx",
            "photo.jpg",
            "archive.zip"
        ]),
        f.names()
    );
    f.app.set_sort(f.tab, "Size").unwrap();
    let names = f.names();
    assert_eq!(strings(&["Pictures", "Documents"]), names[..2].to_vec());
    assert_eq!("archive.zip", names[2]);
    assert_eq!("report.md", names[6]);
    assert!(f.app.tab_info(f.tab).unwrap().sort_desc);

    f.app.set_sort(f.tab, "Name").unwrap();
    assert!(!f.app.tab_info(f.tab).unwrap().sort_desc);
    assert_eq!("Documents", f.names()[0]);
    f.app.set_sort(f.tab, "Name").unwrap();
    assert_eq!(
        strings(&["Pictures", "Documents", "report.md"]),
        f.names()[..3].to_vec()
    );

    f.app.set_sort(f.tab, "Extension").unwrap();
    let names = f.names();
    assert_eq!(strings(&["Documents", "Pictures"]), names[..2].to_vec());
    assert_eq!(
        strings(&["photo.jpg", "report.md"]),
        names[2..4].to_vec(),
        "{names:?}"
    );
}

#[test]
fn an_unusable_sort_column_degrades_to_name() {
    let f = Fixture::new();
    f.app.set_sort(f.tab, "NoSuchColumn").unwrap();
    assert_eq!("Name", f.app.tab_info(f.tab).unwrap().sort_by);
}

#[test]
fn natural_order_reads_numbers_as_numbers() {
    let f = Fixture::new();
    for n in ["img10.txt", "img2.txt", "img1.txt"] {
        std::fs::write(f.root.join(n), b"x").unwrap();
    }
    f.app.refresh(f.tab).unwrap();
    f.settle();
    let names = f.names();
    let at = |n: &str| names.iter().position(|x| x == n).unwrap();
    assert!(at("img1.txt") < at("img2.txt") && at("img2.txt") < at("img10.txt"));
}

// ---- selection ----

#[test]
fn selection_gestures() {
    let f = Fixture::new();
    f.select_names(&["notes.txt", "photo.jpg"]);
    assert_eq!(
        strings(&[&f.path("notes.txt"), &f.path("photo.jpg")]),
        f.app.selected_paths(f.tab).unwrap()
    );
    let archive = f.index_of("archive.zip");
    f.app.select(f.tab, SelectOp::Add, &[archive]).unwrap();
    assert_eq!(3, f.app.tab_info(f.tab).unwrap().selected);
    f.app.select(f.tab, SelectOp::Toggle, &[archive]).unwrap();
    assert_eq!(2, f.app.tab_info(f.tab).unwrap().selected);
    let notes = f.index_of("notes.txt");
    f.app.select(f.tab, SelectOp::Remove, &[notes]).unwrap();
    assert_eq!(
        vec![f.index_of("photo.jpg")],
        f.app.selected_indices(f.tab).unwrap()
    );
    f.app.select(f.tab, SelectOp::Invert, &[]).unwrap();
    assert_eq!(6, f.app.tab_info(f.tab).unwrap().selected);
    f.app.select(f.tab, SelectOp::All, &[]).unwrap();
    assert_eq!(7, f.app.tab_info(f.tab).unwrap().selected);
    f.app.select(f.tab, SelectOp::Clear, &[]).unwrap();
    assert_eq!(0, f.app.tab_info(f.tab).unwrap().selected);
    // An index past the end selects nothing rather than failing.
    f.app.select(f.tab, SelectOp::Replace, &[999]).unwrap();
    assert_eq!(0, f.app.tab_info(f.tab).unwrap().selected);
}

#[test]
fn the_selection_summary_counts_and_totals_what_is_selected() {
    let f = Fixture::new();
    assert_eq!(None, f.app.selection_summary(f.tab).unwrap());
    f.select_names(&["notes.txt"]);
    let notes = std::fs::metadata(f.path("notes.txt")).unwrap().len() as i64;
    assert_eq!(
        Some(format!(
            "1 item selected ({})",
            bb_core::format::byte_size(notes)
        )),
        f.app.selection_summary(f.tab).unwrap()
    );
    f.select_names(&["notes.txt", "photo.jpg"]);
    let photo = std::fs::metadata(f.path("photo.jpg")).unwrap().len() as i64;
    assert_eq!(
        Some(format!(
            "2 items selected ({})",
            bb_core::format::byte_size(notes + photo)
        )),
        f.app.selection_summary(f.tab).unwrap()
    );
    // A folder with no known size adds nothing; on its own there is no total to show.
    f.select_names(&["Pictures"]);
    assert_eq!(
        Some("1 item selected".to_string()),
        f.app.selection_summary(f.tab).unwrap()
    );
}

#[test]
fn selection_is_cleared_by_a_navigation_and_survives_a_resort() {
    let f = Fixture::new();
    f.select_names(&["photo.jpg"]);
    f.app.set_sort(f.tab, "Size").unwrap();
    assert_eq!(
        strings(&[&f.path("photo.jpg")]),
        f.app.selected_paths(f.tab).unwrap()
    );
    f.app.navigate(f.tab, &f.path("Pictures")).unwrap();
    f.settle();
    assert_eq!(0, f.app.tab_info(f.tab).unwrap().selected);
}

#[test]
fn row_ids_are_stable_for_a_path() {
    let f = Fixture::new();
    let id = |f: &Fixture| {
        let listing = f.app.listing(f.tab).unwrap();
        let i = f.index_of("notes.txt") as usize;
        listing.row(i).unwrap().id
    };
    let first = id(&f);
    assert_ne!(0, first);
    f.app.set_sort(f.tab, "Size").unwrap();
    assert_eq!(first, id(&f));
    std::fs::write(f.root.join("zzz.txt"), b"x").unwrap();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    assert_eq!(first, id(&f));
}

// ---- search and flat view ----

#[test]
fn a_folder_search_flattens_and_clearing_it_restores_the_listing() {
    let f = Fixture::new();
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.is_flattened && !info.is_flat_browse);
    assert_eq!(3, info.count, "{:?}", f.names());
    assert!(
        info.status.contains("3 result(s) for 'report'"),
        "{}",
        info.status
    );
    let listing = f.app.listing(f.tab).unwrap();
    assert!(listing.is_flattened);
    // Each hit shows the folder it came from.
    let folders: Vec<String> = listing
        .rows()
        .iter()
        .map(|r| r.relative_dir.clone())
        .collect();
    assert!(
        folders.iter().any(|d| d.is_empty() || d == "."),
        "{folders:?}"
    );
    assert!(
        folders.iter().any(|d| d.contains("Documents")),
        "{folders:?}"
    );

    f.app.clear_search(f.tab).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(!info.is_flattened);
    assert_eq!(7, info.count, "{:?}", f.names());
}

#[test]
fn a_search_with_no_hits_says_so_and_a_bad_query_leaves_the_listing_alone() {
    let f = Fixture::new();
    f.app.set_search(f.tab, "zzzzqqq", false).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert_eq!(0, info.count);
    assert_eq!(
        Some("No results for 'zzzzqqq'".to_string()),
        info.empty_message
    );
    f.app.clear_search(f.tab).unwrap();
    f.settle();
    assert_eq!(7, f.app.tab_info(f.tab).unwrap().count);
}

#[test]
fn flat_view_lists_every_file_and_is_a_browse_not_a_search() {
    let f = Fixture::new();
    f.app.set_flat_view(f.tab, true).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.is_flattened && info.is_flat_browse);
    assert!(info.is_flat_view);
    // Files only: 3 report.md, holiday.png, notes, photo, archive, budget.
    assert_eq!(8, info.count, "{:?}", f.names());
    assert!(info.status.contains("file(s) under"), "{}", info.status);
    let listing = f.app.listing(f.tab).unwrap();
    assert!(listing.is_flat_browse);
    // A flat browse is not a folder listing: no folders band, the Folder column appears.
    let columns = f.app.columns(f.tab).unwrap();
    assert!(columns.iter().any(|c| c.id() == "RelativePath"));

    f.app.set_flat_view(f.tab, false).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(!info.is_flattened);
    assert_eq!(7, info.count, "{:?}", f.names());
}

#[test]
fn flat_view_survives_navigation_and_a_search_wins_over_it() {
    let f = Fixture::new();
    f.app.set_flat_view(f.tab, true).unwrap();
    f.settle();
    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.is_flat_browse, "flat is a mode, not a gesture");
    assert_eq!(2, info.count);
    f.app.set_search(f.tab, "nested", false).unwrap();
    f.settle();
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(info.is_flattened && !info.is_flat_browse);
}

// ---- operations ----

#[test]
fn a_move_and_its_undo_and_redo_round_trip() {
    let f = Fixture::new();
    f.select_names(&["photo.jpg", "archive.zip"]);
    let sources = f.app.selected_paths(f.tab).unwrap();
    let op = f.app.op_move(sources, &f.path("Documents")).unwrap();
    assert!(f.app.is_busy() || f.app.is_transferring() || f.app.op_state(op).is_some());
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(message.starts_with("Moved 2 item(s)"), "{message}");
    // The hint names Undo's key, or says the plain sentence when it has none.
    let gesture = f.app.keymap_gesture("edit.undo");
    let hint = if gesture.is_empty() {
        " \u{2014} this can be undone".to_string()
    } else {
        format!(" \u{2014} {gesture} to undo")
    };
    assert!(message.ends_with(&hint), "{message}");

    assert!(!f.names().contains(&"photo.jpg".to_string()));
    assert!(std::path::Path::new(&f.path("Documents\\photo.jpg")).exists());
    assert!(!std::path::Path::new(&f.path("photo.jpg")).exists());
    assert!(f.app.can_undo());
    assert!(
        f.app
            .undo_description()
            .unwrap()
            .starts_with("Move 2 items to Documents")
    );

    let undo = f.app.undo_step(1).unwrap();
    let (ok, message) = f.wait_op(undo);
    assert!(ok, "{message}");
    assert!(std::path::Path::new(&f.path("photo.jpg")).exists());
    assert!(!std::path::Path::new(&f.path("Documents\\photo.jpg")).exists());
    assert!(f.names().contains(&"photo.jpg".to_string()));
    assert!(!f.app.can_undo() && f.app.can_redo());

    let redo = f.app.undo_step(-1).unwrap();
    let (ok, message) = f.wait_op(redo);
    assert!(ok, "{message}");
    assert!(std::path::Path::new(&f.path("Documents\\photo.jpg")).exists());
    assert!(!f.names().contains(&"photo.jpg".to_string()));
}

#[test]
fn a_copy_leaves_the_source_and_is_not_on_the_undo_history() {
    let f = Fixture::new();
    let op = f
        .app
        .op_copy(vec![f.path("notes.txt")], &f.path("Pictures"))
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(message.starts_with("Copied 1 item(s)"), "{message}");
    assert!(std::path::Path::new(&f.path("Pictures\\notes.txt")).exists());
    assert!(std::path::Path::new(&f.path("notes.txt")).exists());
    assert!(!f.app.can_undo());
}

#[test]
fn a_clash_asks_the_conflict_prompt_and_honours_the_answer() {
    let f = Fixture::new();
    // Documents already has a report.md; moving the root's onto it clashes.
    f.probes.answer_conflict(
        &f.path("report.md"),
        bb_core::transfer::ConflictResolution::Skip,
    );
    let op = f
        .app
        .op_move(vec![f.path("report.md")], &f.path("Documents"))
        .unwrap();
    let (_, message) = f.wait_op(op);
    assert_eq!(1, f.probes.conflict_asks());
    assert!(message.contains("skipped 1"), "{message}");
    assert!(std::path::Path::new(&f.path("report.md")).exists());

    f.probes.cancel_conflicts(true);
    let op = f
        .app
        .op_move(vec![f.path("report.md")], &f.path("Documents"))
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(!ok);
    assert_eq!("cancelled", message);
}

#[test]
fn a_staged_delete_can_be_undone_and_a_permanent_one_cannot() {
    let f = Fixture::new();
    let op = f
        .app
        .op_delete(vec![f.path("budget.xlsx")], DeleteMode::Staged)
        .unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(!std::path::Path::new(&f.path("budget.xlsx")).exists());
    assert!(!f.names().contains(&"budget.xlsx".to_string()));
    assert!(f.app.can_undo());

    let undo = f.app.undo_step(1).unwrap();
    let (ok, message) = f.wait_op(undo);
    assert!(ok, "{message}");
    assert!(std::path::Path::new(&f.path("budget.xlsx")).exists());
    assert!(f.names().contains(&"budget.xlsx".to_string()));

    // Redo deletes it again, then a permanent delete leaves nothing to undo.
    let redo = f.app.undo_step(-1).unwrap();
    f.wait_op(redo);
    assert!(!std::path::Path::new(&f.path("budget.xlsx")).exists());
    let op = f
        .app
        .op_delete(vec![f.path("notes.txt")], DeleteMode::Permanent)
        .unwrap();
    f.wait_op(op);
    assert!(!std::path::Path::new(&f.path("notes.txt")).exists());
    let described = f.app.undo_description().unwrap_or_default();
    assert!(
        described.contains("budget.xlsx"),
        "the permanent delete must not have pushed an entry: {described}"
    );
}

#[test]
fn a_rename_numbers_across_a_selection_and_undoes() {
    let f = Fixture::new();
    f.select_names(&["notes.txt", "report.md"]);
    let sources = f.app.selected_paths(f.tab).unwrap();
    let op = f.app.op_rename(sources, "Holiday").unwrap();
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    let names = f.names();
    assert!(names.contains(&"Holiday 1.txt".to_string()), "{names:?}");
    assert!(names.contains(&"Holiday 2.md".to_string()), "{names:?}");
    assert!(!names.contains(&"notes.txt".to_string()));

    let undo = f.app.undo_step(1).unwrap();
    f.wait_op(undo);
    assert!(f.names().contains(&"notes.txt".to_string()));
    assert!(!f.names().contains(&"Holiday 1.txt".to_string()));
}

#[test]
fn nothing_to_do_is_reported_not_recorded() {
    let f = Fixture::new();
    let op = f
        .app
        .op_move(vec![f.path("notes.txt")], &f.root.to_string_lossy())
        .unwrap();
    let (ok, _) = f.wait_op(op);
    assert!(!ok);
    assert_eq!(
        "Nothing to move \u{2014} those items are already there.",
        f.status()
    );
    assert!(!f.app.can_undo());
    assert!(f.app.undo_step(1).is_err());
}

#[test]
fn the_queue_holds_transferring_while_paused_and_refuses_a_rename() {
    let f = Fixture::new();
    f.app.op_pause(true);
    let op = f
        .app
        .op_copy(vec![f.path("notes.txt")], &f.path("Pictures"))
        .unwrap();
    // Accepted: this app is writing, though the settle predicate treats a pause as quiet.
    assert!(f.app.is_transferring());
    assert!(f.app.is_paused());
    assert!(eventually(2_000, || !f.app.is_busy()));
    assert!(
        f.app.is_transferring(),
        "pausing does not release the queue"
    );
    assert!(matches!(
        f.app.op_rename(vec![f.path("photo.jpg")], "x"),
        Err(AppError::Busy)
    ));
    assert!(!std::path::Path::new(&f.path("Pictures\\notes.txt")).exists());

    f.app.op_pause(false);
    let (ok, message) = f.wait_op(op);
    assert!(ok, "{message}");
    assert!(!f.app.is_transferring());
    assert!(std::path::Path::new(&f.path("Pictures\\notes.txt")).exists());
}

#[test]
fn two_queued_jobs_hold_the_flag_across_the_gap() {
    let f = Fixture::new();
    f.app.op_pause(true);
    let first = f
        .app
        .op_copy(vec![f.path("notes.txt")], &f.path("Pictures"))
        .unwrap();
    let second = f
        .app
        .op_copy(vec![f.path("photo.jpg")], &f.path("Pictures"))
        .unwrap();
    f.app.op_pause(false);
    let mut seen_idle_between = false;
    while !matches!(f.app.op_state(second), Some(bb_app::OpState::Done { .. })) {
        if matches!(f.app.op_state(first), Some(bb_app::OpState::Done { .. }))
            && !f.app.is_transferring()
            && !matches!(f.app.op_state(second), Some(bb_app::OpState::Done { .. }))
        {
            seen_idle_between = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!seen_idle_between, "the flag was released between two jobs");
    f.settle();
    assert!(!f.app.is_transferring());
}

// ---- the watcher ----

#[test]
fn a_file_created_behind_the_apps_back_appears_and_the_selection_survives() {
    let f = Fixture::new();
    f.select_names(&["notes.txt"]);
    let before = {
        let listing = f.app.listing(f.tab).unwrap();
        listing.row(f.index_of("notes.txt") as usize).unwrap().id
    };
    std::fs::write(f.root.join("appeared.txt"), vec![0u8; 1024]).unwrap();
    assert!(
        eventually(10_000, || f.names().contains(&"appeared.txt".to_string())),
        "the watcher never merged the new file: {:?}",
        f.names()
    );
    f.settle();
    assert_eq!(8, f.app.tab_info(f.tab).unwrap().count);
    assert_eq!(
        strings(&[&f.path("notes.txt")]),
        f.app.selected_paths(f.tab).unwrap(),
        "a merge must not drop the selection"
    );
    let after = {
        let listing = f.app.listing(f.tab).unwrap();
        listing.row(f.index_of("notes.txt") as usize).unwrap().id
    };
    assert_eq!(before, after, "row ids are stable across a merge");
    // The new row went in at its sorted place.
    let names = f.names();
    assert_eq!("appeared.txt", names[2], "{names:?}");

    std::fs::remove_file(f.root.join("appeared.txt")).unwrap();
    assert!(eventually(10_000, || !f
        .names()
        .contains(&"appeared.txt".to_string())));
    f.settle();
    assert_eq!(7, f.app.tab_info(f.tab).unwrap().count);
    assert_eq!(1, f.app.tab_info(f.tab).unwrap().selected);
}

#[test]
fn a_changed_file_updates_in_place_and_removing_a_selected_row_drops_it() {
    let f = Fixture::new();
    f.select_names(&["budget.xlsx"]);
    std::fs::write(f.root.join("budget.xlsx"), vec![0u8; 2048]).unwrap();
    assert!(eventually(10_000, || {
        let l = f.app.listing(f.tab).unwrap();
        l.rows()
            .iter()
            .any(|r| r.name == "budget.xlsx" && r.size == Some(2048))
    }));
    assert_eq!(1, f.app.tab_info(f.tab).unwrap().selected);
    std::fs::remove_file(f.root.join("budget.xlsx")).unwrap();
    assert!(eventually(10_000, || !f
        .names()
        .contains(&"budget.xlsx".to_string())));
    assert_eq!(0, f.app.tab_info(f.tab).unwrap().selected);
}

#[test]
fn a_search_result_is_not_merged() {
    let f = Fixture::new();
    f.app.set_search(f.tab, "report", false).unwrap();
    f.settle();
    std::fs::write(f.root.join("other.txt"), b"x").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    f.settle();
    assert_eq!(3, f.app.tab_info(f.tab).unwrap().count);
}

// ---- events and the settle predicate ----

#[test]
fn loading_raises_listing_and_busy_events_in_order() {
    let f = Fixture::new();
    f.events.lock().unwrap().clear();
    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.settle();
    let events = f.events.lock().unwrap().clone();
    let busy: Vec<bool> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::BusyChanged { busy } => Some(*busy),
            _ => None,
        })
        .collect();
    assert_eq!(Some(&true), busy.first(), "{events:?}");
    assert_eq!(Some(&false), busy.last(), "{events:?}");
    let listing = events
        .iter()
        .filter(|e| matches!(e, AppEvent::ListingChanged { tab, .. } if *tab == f.tab))
        .count();
    assert!(listing >= 1, "{events:?}");
    let version = f.app.tab_info(f.tab).unwrap().version;
    assert!(events.contains(&AppEvent::ListingChanged {
        tab: f.tab,
        version
    }));
}

#[test]
fn an_old_snapshot_is_never_mutated() {
    let f = Fixture::new();
    let old = f.app.listing(f.tab).unwrap();
    let old_names: Vec<String> = old.rows().iter().map(|r| r.name.clone()).collect();
    std::fs::write(f.root.join("fresh.txt"), b"x").unwrap();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    let new = f.app.listing(f.tab).unwrap();
    assert!(new.version > old.version);
    assert_eq!(
        old_names,
        old.rows()
            .iter()
            .map(|r| r.name.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(old_names.len() + 1, new.len());
}

// ---- cells ----

#[test]
fn cells_format_sizes_dates_types_and_attributes() {
    let f = Fixture::new();
    let listing = f.app.listing(f.tab).unwrap();
    let cell = |name: &str, column: &str| -> String {
        let i = f.index_of(name) as usize;
        let c = bb_app::cell_index(column).unwrap();
        String::from_utf16(listing.cell(i, c).unwrap()).unwrap()
    };
    assert_eq!("notes.txt", cell("notes.txt", "Name"));
    assert_eq!("100 B", cell("notes.txt", "Size"));
    assert_eq!("19.5 KB", cell("archive.zip", "Size"));
    assert_eq!("TXT file", cell("notes.txt", "Type"));
    assert_eq!("txt", cell("notes.txt", "Extension"));
    assert_eq!("Folder", cell("Documents", "Type"));
    assert_eq!(
        "\u{2014}",
        cell("Documents", "Size"),
        "an unmeasured folder is a dash, never zero"
    );
    let modified = cell("notes.txt", "Modified");
    assert_eq!(16, modified.len(), "{modified}");
    assert!(cell("notes.txt", "Attributes").contains('A'));
    assert_eq!("", cell("notes.txt", "RelativePath"));
}

#[test]
fn a_folder_size_comes_from_the_cache_and_only_from_the_cache() {
    let f = Fixture::new();
    let db = bb_core::data::Db::new(f.data.join("bertbrowser.db"));
    let repo = bb_core::data::DirSizeRepository::new(std::sync::Arc::new(db));
    repo.upsert_many(&[bb_core::data::DirSizeResult {
        path_key: bb_paths::PathKey::canonicalize(&f.path("Documents")).unwrap(),
        size_bytes: 12345,
        file_count: 2,
        dir_count: 1,
        incomplete: true,
        computed_utc: bb_core::FileTime::now(),
    }])
    .unwrap();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    let listing = f.app.listing(f.tab).unwrap();
    let docs = listing.row(f.index_of("Documents") as usize).unwrap();
    assert_eq!(Some(12345), docs.size);
    assert!(docs.size_incomplete);
    let pictures = listing.row(f.index_of("Pictures") as usize).unwrap();
    assert_eq!(None, pictures.size);
}

// ---- seams ----

#[test]
fn entering_a_file_reaches_the_refusing_launcher_and_says_so() {
    let f = Fixture::new();
    // An archive's bytes are read off the calling thread before it is entered or handed on.
    f.app.open_item(f.tab, f.index_of("archive.zip")).unwrap();
    f.settle();
    assert_eq!(vec![(f.path("archive.zip"), false)], f.probes.launches());
    assert!(f.status().contains("was not launched"), "{}", f.status());
    // A folder opens in place and launches nothing.
    f.app.open_item(f.tab, f.index_of("Documents")).unwrap();
    f.settle();
    assert_eq!(f.path("Documents"), f.app.tab_info(f.tab).unwrap().path);
    assert_eq!(1, f.probes.launches().len());
}

#[test]
fn a_huge_flat_view_asks_before_it_walks() {
    let f = Fixture::new();
    // The estimate comes from dir_size_cache: claim the root is enormous.
    let db = bb_core::data::Db::new(f.data.join("bertbrowser.db"));
    let repo = bb_core::data::DirSizeRepository::new(std::sync::Arc::new(db));
    repo.upsert_many(&[bb_core::data::DirSizeResult {
        path_key: bb_paths::PathKey::canonicalize(&f.root.to_string_lossy()).unwrap(),
        size_bytes: 1 << 40,
        file_count: 5_000_000,
        dir_count: 10,
        incomplete: false,
        computed_utc: bb_core::FileTime::now(),
    }])
    .unwrap();
    f.probes.set_confirm_answer(false);
    f.app.set_flat_view(f.tab, true).unwrap();
    f.settle();
    assert_eq!(1, f.probes.confirms().len());
    assert!(
        f.probes.confirms()[0].contains("5,000,000"),
        "{:?}",
        f.probes.confirms()
    );
    let info = f.app.tab_info(f.tab).unwrap();
    assert!(
        !info.is_flat_view && !info.is_flattened,
        "declining must leave the folder showing"
    );
}

// ---- panes and tabs ----

#[test]
fn panes_split_close_and_the_layout_is_reported() {
    let f = Fixture::new();
    let first = f.app.active_pane();
    let second = f
        .app
        .split_pane(first, SplitOrientation::Vertical, &f.path("Pictures"))
        .unwrap();
    f.settle();
    assert_eq!(vec![first, second], f.app.panes());
    assert_eq!(second, f.app.active_pane());
    let tab = f.app.active_tab().unwrap();
    assert_eq!(f.path("Pictures"), f.app.tab_info(tab).unwrap().path);

    let state: serde_json::Value = serde_json::from_str(&f.app.state_json()).unwrap();
    assert_eq!("split", state["layout"]["kind"]);
    assert_eq!("vertical", state["layout"]["orientation"]);
    assert_eq!(2, state["layout"]["children"].as_array().unwrap().len());
    assert_eq!(2, state["panes"].as_array().unwrap().len());

    f.app.activate_pane(first).unwrap();
    f.app.close_pane(second).unwrap();
    assert_eq!(vec![first], f.app.panes());
    assert!(matches!(f.app.tab_info(tab), Err(AppError::Stale)));
    assert!(
        f.app.close_pane(first).is_err(),
        "the last pane cannot be closed"
    );
}

#[test]
fn tabs_open_activate_and_close_with_a_neighbour_taking_over() {
    let f = Fixture::new();
    let pane = f.app.active_pane();
    let second = f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    f.settle();
    assert_eq!(vec![f.tab, second], f.app.pane_tabs(pane).unwrap());
    assert_eq!(Some(second), f.app.active_tab());
    f.app.activate_tab(f.tab).unwrap();
    assert_eq!(Some(f.tab), f.app.active_tab());
    f.app.close_tab(f.tab).unwrap();
    assert_eq!(Some(second), f.app.active_tab());
    // The last tab closes its pane, which cannot be the last pane.
    assert!(f.app.close_tab(second).is_err());
}

#[test]
fn a_new_tab_starts_as_its_neighbour_was_arranged() {
    let f = Fixture::new();
    f.app.set_sort(f.tab, "Size").unwrap();
    let second = f.app.open_tab(None, &f.path("Documents"), true).unwrap();
    f.settle();
    assert_eq!("Size", f.app.tab_info(second).unwrap().sort_by);
}

// ---- commands, keys, settings, theme ----

#[test]
fn keys_resolve_to_commands_and_commands_explain_themselves() {
    let f = Fixture::new();
    let list = FocusState::new(FocusKind::FileList);
    let f5 = KeyChord::parse("F5");
    assert_eq!(
        Some("nav.refresh".to_string()),
        f.app.key_dispatch(&f5, &list)
    );
    let typing = FocusState::new(FocusKind::TextEditable);
    let backspace = KeyChord::parse("Backspace");
    assert_eq!(None, f.app.key_dispatch(&backspace, &typing));

    assert_eq!(
        Some("There is nowhere to go back to.".to_string()),
        f.app.command_why_not("nav.back").unwrap()
    );
    assert_eq!(None, f.app.command_why_not("nav.up").unwrap());
    assert!(f.app.command_why_not("no.such.command").is_err());
    // A window command is gated here and carried out by the host.
    assert_eq!(None, f.app.command_why_not("tools.duplicates").unwrap());
    assert_eq!(
        bb_app::CommandOwner::Ui,
        bb_app::command_owner("tools.duplicates")
    );
    assert_eq!(
        Some("Nothing is selected.".to_string()),
        f.app.command_why_not("file.delete").unwrap()
    );

    f.app.navigate(f.tab, &f.path("Documents")).unwrap();
    f.settle();
    f.app.command_run("nav.back").unwrap();
    f.settle();
    assert_eq!(
        f.root.to_string_lossy(),
        f.app.tab_info(f.tab).unwrap().path
    );
    f.app.command_run("nav.forward").unwrap();
    f.settle();
    f.app.command_run("nav.up").unwrap();
    f.settle();
    assert_eq!(
        f.root.to_string_lossy(),
        f.app.tab_info(f.tab).unwrap().path
    );
    assert!(f.app.command_run("nav.back").is_err() || f.app.tab_info(f.tab).unwrap().can_forward);

    f.app.command_run("select.all").unwrap();
    assert_eq!(7, f.app.tab_info(f.tab).unwrap().selected);
    f.app.command_run("select.none").unwrap();
    f.app.command_run("tab.new").unwrap();
    f.settle();
    assert_eq!(2, f.app.pane_tabs(f.app.active_pane()).unwrap().len());
}

#[test]
fn a_command_delete_goes_through_the_gate_and_undo_command_follows() {
    let f = Fixture::new();
    f.select_names(&["notes.txt"]);
    assert_eq!(None, f.app.command_why_not("file.delete").unwrap());
    assert_eq!(
        Some("There is nothing to undo.".to_string()),
        f.app.command_why_not("edit.undo").unwrap()
    );
}

#[test]
fn settings_round_trip_and_hidden_items_refresh_every_tab() {
    let f = Fixture::new();
    let hidden = f.root.join("secret.txt");
    std::fs::write(&hidden, b"x").unwrap();
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = hidden.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated path.
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(
                wide.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN,
            )
        };
    }
    f.app.refresh(f.tab).unwrap();
    f.settle();
    assert!(!f.names().contains(&"secret.txt".to_string()));

    assert_eq!("false", f.app.settings_get("showHiddenItems").unwrap());
    f.app.settings_set("showHiddenItems", "true").unwrap();
    f.settle();
    assert_eq!("true", f.app.settings_get("showHiddenItems").unwrap());
    assert!(f.names().contains(&"secret.txt".to_string()));
    f.app.settings_flush();
    assert!(
        f.data.join("settings.json").exists(),
        "a change is written on the debounce, and at once when flushed"
    );

    assert!(f.app.settings_set("noSuchSetting", "1").is_err());
    assert!(
        f.app
            .settings_set("showHiddenItems", "\"not a bool\"")
            .is_err()
    );
    assert_eq!("true", f.app.settings_get("showHiddenItems").unwrap());
    assert_eq!("null", f.app.settings_get("themeId").unwrap());
}

#[test]
fn the_palette_has_one_colour_per_token_and_follows_the_system_appearance() {
    let f = Fixture::new();
    let palette = f.app.theme_palette();
    assert_eq!(bb_core::theming::ThemeToken::COUNT, palette.len());

    // A fresh data directory follows Windows: dark, then light when the appearance changes.
    let dark = f.app.theme_palette();
    f.probes
        .set_appearance(bb_core::theming::SystemAppearance::LIGHT);
    let light = f.app.theme_palette();
    assert_ne!(dark, light);
    // Pinning a theme stops following.
    f.app.settings_set("followSystemTheme", "false").unwrap();
    f.app.settings_set("themeId", "\"dark-plus\"").unwrap();
    assert_eq!(dark, f.app.theme_palette());
}

#[test]
fn state_json_carries_the_fields_the_state_verb_reads() {
    let f = Fixture::new();
    f.select_names(&["notes.txt"]);
    let state: serde_json::Value = serde_json::from_str(&f.app.state_json()).unwrap();
    assert_eq!(false, state["busy"]);
    assert_eq!(false, state["transferring"]);
    assert_eq!(f.tab.0, state["activeTab"].as_u64().unwrap());
    let tab = &state["tabs"][0];
    assert_eq!(f.root.to_string_lossy(), tab["path"].as_str().unwrap());
    assert_eq!(7, tab["count"]);
    assert_eq!(1, tab["selected"]);
    assert_eq!(false, tab["flattened"]);
}

// ---- the bench-listing-10k shape ----

#[test]
fn ten_thousand_files_list_select_and_relist() {
    let f = Fixture::new();
    let flat = f.root.join("Flat");
    std::fs::create_dir_all(&flat).unwrap();
    for i in 0..10_000 {
        std::fs::write(flat.join(format!("file{i:05}.dat")), b"x").unwrap();
    }
    let started = std::time::Instant::now();
    f.app.navigate(f.tab, &flat.to_string_lossy()).unwrap();
    f.settle();
    let listed = started.elapsed();
    assert_eq!(10_000, f.app.tab_info(f.tab).unwrap().count);
    assert_eq!("file00000.dat", f.names()[0]);

    // One gesture selects all of them, however many.
    let indices: Vec<u32> = (0..10_000).collect();
    f.app.select(f.tab, SelectOp::Replace, &indices).unwrap();
    assert_eq!(10_000, f.app.tab_info(f.tab).unwrap().selected);

    let started = std::time::Instant::now();
    f.app.refresh(f.tab).unwrap();
    f.settle();
    f.app.up(f.tab).unwrap();
    f.settle();
    f.app.back(f.tab).unwrap();
    f.settle();
    let relisted = started.elapsed();
    assert_eq!(10_000, f.app.tab_info(f.tab).unwrap().count);
    eprintln!("10k: first list {listed:?}, refresh+up+back {relisted:?}");
    assert!(listed.as_secs() < 5 && relisted.as_secs() < 10);
}
