// Port of tests/BertBrowser.Core.Tests/TransferMergeExpanderTests.cs. What a folder landing on a
// folder of the same name expands into. Everything here runs against a fake listing rather than a
// real tree, so the rules can be stated at sizes and shapes (a junction on the destination side,
// a two-thousand-folder overlap) that are impractical to build on disk.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::transfer::{
    ConflictDefaults, ConflictResolution, PlannedTransfer, TransferEntry, TransferEntrySource,
    TransferMergeExpander, TransferMergeLimits, TransferPlan, TransferProbe, TransferVerb,
};
use bb_paths::{fold_upper, parent_key};

const SRC: &str = r"C:\src";
const DST: &str = r"C:\dst";

fn jan() -> FileTime {
    FileTime::from_ymd_hms(2026, 1, 1, 0, 0, 0)
}

/// A filesystem described rather than created. Registering a path registers its ancestors as
/// directories, so a test reads as the tree it means.
#[derive(Default, Clone)]
struct FakeEntrySource {
    by_path: Arc<Mutex<BTreeMap<String, TransferEntry>>>,
}

impl FakeEntrySource {
    fn dir(&self, path: &str) -> &Self {
        self.add(path, true, 0, FileTime::UNKNOWN, false);
        self
    }

    fn link_dir(&self, path: &str) -> &Self {
        self.add(path, true, 0, FileTime::UNKNOWN, true);
        self
    }

    fn file(&self, path: &str) -> &Self {
        self.file_sized(path, 1, jan())
    }

    fn file_sized(&self, path: &str, size: i64, modified: FileTime) -> &Self {
        self.add(path, false, size, modified, false);
        self
    }

    fn add(&self, path: &str, is_directory: bool, size: i64, modified: FileTime, link: bool) {
        if let Some(parent) = parent_key(path)
            && !parent.is_empty()
            && !self
                .by_path
                .lock()
                .unwrap()
                .contains_key(&fold_upper(&parent))
        {
            self.add(&parent, true, 0, FileTime::UNKNOWN, false);
        }
        self.by_path.lock().unwrap().insert(
            fold_upper(path),
            TransferEntry {
                name: bb_paths::file_name(path).to_string(),
                full_path: path.to_string(),
                is_directory,
                is_reparse_point: link,
                size_bytes: size,
                modified_utc: modified,
            },
        );
    }
}

impl TransferProbe for FakeEntrySource {
    fn directory_exists(&self, path: &str) -> bool {
        match self.by_path.lock().unwrap().get(&fold_upper(path)) {
            Some(e) => e.is_directory,
            None => parent_key(path).is_none(),
        }
    }

    fn file_exists(&self, path: &str) -> bool {
        self.by_path
            .lock()
            .unwrap()
            .get(&fold_upper(path))
            .is_some_and(|e| !e.is_directory)
    }

    fn resolve_final_path(&self, path: &str) -> String {
        path.to_string()
    }
}

impl TransferEntrySource for FakeEntrySource {
    fn entries(&self, directory: &str) -> Vec<TransferEntry> {
        let key = fold_upper(directory);
        self.by_path
            .lock()
            .unwrap()
            .values()
            .filter(|e| parent_key(&e.full_path).is_some_and(|p| fold_upper(&p) == key))
            .cloned()
            .collect()
    }
}

fn folder(name: &str) -> PlannedTransfer {
    PlannedTransfer::new(
        &format!(r"{SRC}\{name}"),
        true,
        &format!(r"{DST}\{name}"),
        true,
    )
}

fn plan(transfers: Vec<PlannedTransfer>) -> TransferPlan {
    TransferPlan::new(TransferVerb::Copy, DST, transfers, Vec::new())
}

fn expand_plan(world: &FakeEntrySource, plan: TransferPlan) -> TransferPlan {
    TransferMergeExpander::new(Box::new(world.clone()))
        .expand(plan, &CancelToken::none())
        .unwrap()
}

fn expand(world: &FakeEntrySource, transfer: PlannedTransfer) -> TransferPlan {
    expand_plan(world, plan(vec![transfer]))
}

fn single(plan: &TransferPlan) -> &PlannedTransfer {
    assert_eq!(plan.transfers.len(), 1, "expected one transfer");
    &plan.transfers[0]
}

// --- the shape of an expansion ---

#[test]
fn a_folder_clashing_with_a_folder_gets_no_row_of_its_own() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").file(r"C:\src\photos\a.jpg");
    world.dir(r"C:\dst\photos").file(r"C:\dst\photos\a.jpg");

    let expanded = expand(&world, folder("photos"));

    assert!(
        !expanded
            .transfers
            .iter()
            .any(|t| t.source_path == r"C:\src\photos")
    );
    assert_eq!(vec!["photos".to_string()], expanded.merged_folders);
    assert!(expanded.is_merged());
}

#[test]
fn only_the_descendants_that_clash_are_asked_about() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file(r"C:\src\photos\clashes.jpg")
        .file(r"C:\src\photos\brand-new.jpg");
    world
        .dir(r"C:\dst\photos")
        .file(r"C:\dst\photos\clashes.jpg");

    let expanded = expand(&world, folder("photos"));

    assert_eq!(2, expanded.transfers.len());
    let conflicts = expanded.conflicts();
    assert_eq!(1, conflicts.len());
    assert_eq!(r"C:\src\photos\clashes.jpg", conflicts[0].source_path);
}

#[test]
fn a_child_with_no_twin_is_one_whole_item_and_is_not_descended_into() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .file(r"C:\src\photos\2024\a.jpg")
        .file(r"C:\src\photos\2024\b.jpg");
    world.dir(r"C:\dst\photos");

    let expanded = expand(&world, folder("photos"));

    let only = single(&expanded);
    assert_eq!(r"C:\src\photos\2024", only.source_path);
    assert!(only.is_directory);
    assert!(!only.conflicts);
}

#[test]
fn two_directories_on_both_sides_are_opened_rather_than_asked_about() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .file(r"C:\src\photos\2024\a.jpg");
    world
        .dir(r"C:\dst\photos")
        .dir(r"C:\dst\photos\2024")
        .file(r"C:\dst\photos\2024\a.jpg");

    let expanded = expand(&world, folder("photos"));

    let only = single(&expanded);
    assert_eq!(r"C:\src\photos\2024\a.jpg", only.source_path);
    assert!(only.conflicts);
}

#[test]
fn every_emitted_destination_has_a_parent_that_already_exists() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .dir(r"C:\src\photos\2024\raw")
        .file(r"C:\src\photos\2024\raw\a.dng")
        .file(r"C:\src\photos\2024\b.jpg");
    world
        .dir(r"C:\dst\photos")
        .dir(r"C:\dst\photos\2024")
        .dir(r"C:\dst\photos\2024\raw")
        .file(r"C:\dst\photos\2024\raw\a.dng");

    let expanded = expand(&world, folder("photos"));

    assert!(!expanded.transfers.is_empty());
    for transfer in &expanded.transfers {
        let parent = parent_key(&transfer.destination_path).unwrap();
        assert!(
            world.directory_exists(&parent),
            "nothing may have to invent {parent}"
        );
    }
}

#[test]
fn a_nested_overlap_expands_through_every_common_level() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .file(r"C:\src\photos\2024\deep.jpg");
    world
        .dir(r"C:\dst\photos")
        .dir(r"C:\dst\photos\2024")
        .file(r"C:\dst\photos\2024\deep.jpg");

    let expanded = expand(&world, folder("photos"));

    // Relative to where the drop lands, not to the merged folder, so a row names the folder it
    // came from as well as the file.
    assert_eq!(
        r"photos\2024\deep.jpg",
        expanded.label_for(single(&expanded))
    );
}

// --- what is not a merge ---

#[test]
fn a_folder_arriving_where_a_file_sits_is_one_clashing_leaf() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").file(r"C:\src\photos\a.jpg");
    world.file(r"C:\dst\photos");

    let expanded = expand(&world, folder("photos"));

    let only = single(&expanded);
    assert_eq!(r"C:\src\photos", only.source_path);
    assert!(only.conflicts);
    assert!(!expanded.is_merged());
}

#[test]
fn a_file_arriving_where_a_folder_sits_is_one_clashing_leaf() {
    let world = FakeEntrySource::default();
    world
        .file(r"C:\src\notes")
        .dir(r"C:\dst\notes")
        .file(r"C:\dst\notes\inner.txt");

    let expanded = expand(
        &world,
        PlannedTransfer::new(r"C:\src\notes", false, r"C:\dst\notes", true),
    );

    assert_eq!(1, expanded.transfers.len());
    assert!(!expanded.is_merged());
}

#[test]
fn a_file_clash_with_no_folder_involved_is_left_alone_but_described() {
    let world = FakeEntrySource::default();
    world
        .file_sized(r"C:\src\a.txt", 10, jan())
        .file_sized(r"C:\dst\a.txt", 20, jan());

    let expanded = expand(
        &world,
        PlannedTransfer::new(r"C:\src\a.txt", false, r"C:\dst\a.txt", true),
    );

    let only = single(&expanded);
    let clash = only.clash.expect("described");
    assert_eq!(20, clash.existing.unwrap().bytes);
}

#[test]
fn a_plan_with_nothing_clashing_is_returned_untouched() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").dir(r"C:\dst");
    let plan = plan(vec![PlannedTransfer::new(
        r"C:\src\photos",
        true,
        r"C:\dst\photos",
        false,
    )]);

    assert_eq!(plan.clone(), expand_plan(&world, plan));
}

// --- links are always leaves ---

#[test]
fn a_source_folder_that_is_a_junction_is_never_descended_into() {
    let world = FakeEntrySource::default();
    world
        .link_dir(r"C:\src\photos")
        .file(r"C:\src\photos\a.jpg");
    world.dir(r"C:\dst\photos").file(r"C:\dst\photos\a.jpg");

    let expanded = expand(&world, folder("photos"));

    assert_eq!(r"C:\src\photos", single(&expanded).source_path);
    assert!(!expanded.is_merged());
}

/// The dangerous one. A junction at the destination pointing back into the source would be merged
/// into one file at a time, by a path `Revalidate` never inspects: it compares a source against
/// the plan's destination directory, and an expanded source is always deeper.
#[test]
fn a_destination_folder_that_is_a_junction_is_never_merged_into() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").file(r"C:\src\photos\a.jpg");
    world
        .link_dir(r"C:\dst\photos")
        .file(r"C:\dst\photos\a.jpg");

    let expanded = expand(&world, folder("photos"));

    assert_eq!(r"C:\src\photos", single(&expanded).source_path);
    assert!(!expanded.is_merged());
}

#[test]
fn a_junction_inside_a_merged_folder_is_an_item_rather_than_a_subtree() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .link_dir(r"C:\src\photos\link")
        .file(r"C:\src\photos\link\a.jpg");
    world.dir(r"C:\dst\photos").dir(r"C:\dst\photos\link");

    let expanded = expand(&world, folder("photos"));

    let only = single(&expanded);
    assert_eq!(r"C:\src\photos\link", only.source_path);
    assert!(only.conflicts);
}

// --- two folders merging into one ---

#[test]
fn two_folders_of_the_same_name_merging_into_one_flag_the_second_claimant() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\a\photos").file(r"C:\a\photos\x.jpg");
    world.dir(r"C:\b\photos").file(r"C:\b\photos\x.jpg");
    world.dir(r"C:\dst\photos");

    let plan = plan(vec![
        PlannedTransfer::new(r"C:\a\photos", true, r"C:\dst\photos", true),
        PlannedTransfer::new(r"C:\b\photos", true, r"C:\dst\photos", true),
    ]);

    let expanded = expand_plan(&world, plan);

    assert_eq!(2, expanded.transfers.len());
    assert!(!expanded.transfers[0].conflicts);
    assert!(expanded.transfers[1].conflicts);
    assert!(
        expanded
            .earlier_claimant_of(&expanded.transfers[1])
            .is_some()
    );
}

// --- pruning ---

#[test]
fn every_opened_source_folder_is_listed_for_pruning_deepest_first() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .file(r"C:\src\photos\2024\a.jpg");
    world
        .dir(r"C:\dst\photos")
        .dir(r"C:\dst\photos\2024")
        .file(r"C:\dst\photos\2024\a.jpg");

    let expanded = expand(&world, folder("photos"));

    assert_eq!(
        vec![
            r"C:\src\photos\2024".to_string(),
            r"C:\src\photos".to_string()
        ],
        expanded.prune_directories
    );
}

#[test]
fn a_folder_that_was_not_merged_contributes_nothing_to_pruning() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").file(r"C:\src\photos\a.jpg");
    world.file(r"C:\dst\photos");

    assert!(
        expand(&world, folder("photos"))
            .prune_directories
            .is_empty()
    );
}

// --- identical ---

fn noon() -> FileTime {
    FileTime::from_ymd_hms(2026, 3, 1, 12, 0, 0)
}

#[test]
fn a_byte_identical_clash_is_marked_identical_and_defaults_to_skip() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 900, noon());
    world
        .dir(r"C:\dst\photos")
        .file_sized(r"C:\dst\photos\a.jpg", 900, noon());

    let expanded = expand(&world, folder("photos"));
    let only = single(&expanded);

    assert!(only.clash.unwrap().identical);
    assert_eq!(
        ConflictResolution::Skip,
        ConflictDefaults::for_transfer(only)
    );
}

#[test]
fn a_different_size_at_the_same_instant_is_not_identical() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 900, noon());
    world
        .dir(r"C:\dst\photos")
        .file_sized(r"C:\dst\photos\a.jpg", 901, noon());

    let expanded = expand(&world, folder("photos"));
    let only = single(&expanded);

    assert!(!only.clash.unwrap().identical);
    assert_eq!(
        ConflictResolution::KeepBoth,
        ConflictDefaults::for_transfer(only)
    );
}

/// FAT and exFAT round a write time to two seconds, so a file copied to a USB stick comes back up
/// to that much off and must not read as "newer" for ever.
#[test]
fn two_seconds_apart_at_the_same_size_is_still_identical() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 900, noon().add_secs(2));
    world
        .dir(r"C:\dst\photos")
        .file_sized(r"C:\dst\photos\a.jpg", 900, noon());

    assert!(
        single(&expand(&world, folder("photos")))
            .clash
            .unwrap()
            .identical
    );
}

/// The load-bearing one. A folder comparison forgives a whole-hour shift, because FAT stores local
/// time with no zone. Forgiving it here would pre-select Skip on a file that really is an hour
/// newer: the user asks for a merge and silently does not get one.
#[test]
fn a_whole_hour_newer_is_not_identical_and_is_reported_as_newer() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 900, noon().add_hours(1));
    world
        .dir(r"C:\dst\photos")
        .file_sized(r"C:\dst\photos\a.jpg", 900, noon());

    let expanded = expand(&world, folder("photos"));
    let clash = single(&expanded).clash.unwrap();

    assert!(!clash.identical);
    assert!(clash.incoming_is_newer);
}

#[test]
fn a_missing_timestamp_is_never_identical() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 900, FileTime::UNKNOWN);
    world
        .dir(r"C:\dst\photos")
        .file_sized(r"C:\dst\photos\a.jpg", 900, FileTime::UNKNOWN);

    assert!(
        !single(&expand(&world, folder("photos")))
            .clash
            .unwrap()
            .identical
    );
}

// --- sizes come free ---

#[test]
fn an_expanded_file_carries_the_size_the_walk_already_read() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .file_sized(r"C:\src\photos\a.jpg", 4096, jan());
    world.dir(r"C:\dst\photos");

    assert_eq!(
        Some(4096),
        single(&expand(&world, folder("photos"))).known_bytes
    );
}

#[test]
fn an_expanded_folder_carries_no_size_because_that_comes_from_the_index() {
    let world = FakeEntrySource::default();
    world
        .dir(r"C:\src\photos")
        .dir(r"C:\src\photos\2024")
        .file(r"C:\src\photos\2024\a.jpg");
    world.dir(r"C:\dst\photos");

    assert_eq!(None, single(&expand(&world, folder("photos"))).known_bytes);
}

// --- the ceilings ---

#[test]
fn over_the_item_ceiling_the_folder_keeps_its_wholesale_question() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").dir(r"C:\dst\photos");
    for i in 0..=TransferMergeLimits::MAX_EXPANDED_TRANSFERS {
        world.file(&format!(r"C:\src\photos\f{i}.txt"));
    }

    let expanded = expand(&world, folder("photos"));

    let only = single(&expanded);
    assert_eq!(r"C:\src\photos", only.source_path);
    assert!(only.merge_declined);
    assert!(!expanded.is_merged());
    assert!(expanded.prune_directories.is_empty());
}

#[test]
fn over_the_directory_ceiling_the_folder_keeps_its_wholesale_question() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").dir(r"C:\dst\photos");
    for i in 0..=TransferMergeLimits::MAX_DIRECTORIES_VISITED {
        world
            .dir(&format!(r"C:\src\photos\d{i}"))
            .dir(&format!(r"C:\dst\photos\d{i}"));
        world.file(&format!(r"C:\src\photos\d{i}\f.txt"));
    }

    assert!(single(&expand(&world, folder("photos"))).merge_declined);
}

#[test]
fn one_folder_may_decline_while_another_merges() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\big").dir(r"C:\dst\big");
    world.dir(r"C:\src\small").file(r"C:\src\small\a.txt");
    world.dir(r"C:\dst\small").file(r"C:\dst\small\a.txt");
    for i in 0..=TransferMergeLimits::MAX_EXPANDED_TRANSFERS {
        world.file(&format!(r"C:\src\big\f{i}.txt"));
    }

    let expanded = expand_plan(
        &world,
        plan(vec![
            PlannedTransfer::new(r"C:\src\big", true, r"C:\dst\big", true),
            PlannedTransfer::new(r"C:\src\small", true, r"C:\dst\small", true),
        ]),
    );

    assert_eq!(vec!["small".to_string()], expanded.merged_folders);
    assert!(
        expanded
            .transfers
            .iter()
            .any(|t| t.source_path == r"C:\src\big" && t.merge_declined)
    );
    assert!(
        expanded
            .transfers
            .iter()
            .any(|t| t.source_path == r"C:\src\small\a.txt")
    );
}

// --- degenerate input ---

#[test]
fn an_unreadable_folder_becomes_a_leaf_rather_than_throwing() {
    let world = FakeEntrySource::default();
    world.dir(r"C:\src\photos").dir(r"C:\dst\photos");

    // Nothing inside either side: the walk opens both, finds nothing, and emits nothing.
    let expanded = expand(&world, folder("photos"));

    assert!(expanded.transfers.is_empty());
    assert_eq!(
        vec![r"C:\src\photos".to_string()],
        expanded.prune_directories
    );
}
