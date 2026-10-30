// Port of tests/BertBrowser.Core.Tests/ArchiveEditTests.cs. Editing a container in place, which
// is really rewriting it beside itself and swapping. The happy path matters least here. What these
// are for is the safety: that a refusal is a refusal rather than a silent conversion, that a
// failure leaves the original exactly where it was, and that undo puts back the same bytes.
use bb_core::archives::{
    ArchiveEdit, ArchiveEditExecutor, ArchiveEditOutcome, ArchiveEditPlan, ArchiveEditPlanner,
    ArchiveEditRejection, ArchiveReader, ArchiveType, CompressionType, LibraryArchiveReader,
};
use bb_core::cancel::CancelToken;
use bb_core::rename::RenamePattern;
use bb_test_support::archive_fixtures;
use bb_test_support::archive_support::{TempRoot, write_archive, write_zip};

struct Fixture {
    root: TempRoot,
    reader: LibraryArchiveReader,
    planner: ArchiveEditPlanner,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            root: TempRoot::new("bertbrowser-archedit"),
            reader: LibraryArchiveReader::new(),
            planner: ArchiveEditPlanner::new(),
        }
    }

    fn executor(&self) -> ArchiveEditExecutor<'_> {
        ArchiveEditExecutor::new(&self.reader)
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> String {
        write_zip(&self.root.path(name), entries)
    }

    fn write(
        &self,
        name: &str,
        kind: ArchiveType,
        compression: CompressionType,
        entries: &[(&str, &[u8])],
    ) -> String {
        write_archive(&self.root.path(name), kind, compression, entries)
    }

    fn plan(&self, archive: &str, edits: &[ArchiveEdit]) -> ArchiveEditPlan {
        let length = std::fs::metadata(archive).unwrap().len() as i64;
        self.planner.plan(
            &self.reader.read(archive, None, &CancelToken::none()),
            archive,
            length,
            edits,
        )
    }

    fn execute(&self, plan: &ArchiveEditPlan) -> ArchiveEditOutcome {
        self.executor()
            .execute(plan, &CancelToken::none(), None, None)
    }

    fn entries_of(&self, archive: &str) -> Vec<String> {
        let mut paths: Vec<String> = self
            .reader
            .read(archive, None, &CancelToken::none())
            .nodes()
            .filter(|n| !n.is_directory)
            .map(|n| n.path.clone())
            .collect();
        paths.sort();
        paths
    }

    fn body(&self, archive: &str, entry: &str) -> String {
        let bytes = self
            .reader
            .read_entry(archive, entry, 4096, None, &CancelToken::none())
            .expect("entry");
        String::from_utf8(bytes).unwrap()
    }

    fn leftover_rewrites(&self) -> Vec<String> {
        std::fs::read_dir(&self.root.root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(ArchiveEditExecutor::REWRITE_MARKER))
            .collect()
    }
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

// --- what it does ---

#[test]
fn removing_an_entry_rewrites_without_it() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("keep.txt", b"kept"),
            ("drop.txt", b"gone"),
            ("src/app.js", b"code"),
        ],
    );

    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("drop.txt")]));

    assert_eq!(None, outcome.failure);
    assert_eq!(vec!["keep.txt", "src\\app.js"], f.entries_of(&zip));
    assert_eq!("kept", f.body(&zip, "keep.txt"));
    assert_eq!("code", f.body(&zip, "src\\app.js"));
}

#[test]
fn removing_a_folder_takes_everything_under_it() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[
            ("keep.txt", b"k"),
            ("src/app.js", b"a"),
            ("src/lib/util.js", b"u"),
        ],
    );

    f.execute(&f.plan(&zip, &[ArchiveEdit::remove("src")]));

    assert_eq!(vec!["keep.txt"], f.entries_of(&zip));
}

#[test]
fn renaming_an_entry_keeps_its_contents() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("old.txt", b"unchanged"), ("other.txt", b"x")]);

    f.execute(&f.plan(&zip, &[ArchiveEdit::rename("old.txt", "new.txt")]));

    assert_eq!(vec!["new.txt", "other.txt"], f.entries_of(&zip));
    assert_eq!("unchanged", f.body(&zip, "new.txt"));
}

#[test]
fn renaming_a_folder_moves_everything_beneath_it() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("src/app.js", b"a"), ("src/lib/util.js", b"u")]);

    f.execute(&f.plan(&zip, &[ArchiveEdit::rename("src", "source")]));

    assert_eq!(
        vec!["source\\app.js", "source\\lib\\util.js"],
        f.entries_of(&zip)
    );
}

#[test]
fn adding_a_file_puts_it_in() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("there.txt", b"x")]);
    let extra = f.root.make("extra.txt", b"brand new");

    f.execute(&f.plan(&zip, &[ArchiveEdit::add_file(&extra, "extra.txt")]));

    assert_eq!(vec!["extra.txt", "there.txt"], f.entries_of(&zip));
    assert_eq!("brand new", f.body(&zip, "extra.txt"));
}

#[test]
fn tar_family_rewrites_to_the_same_kind_of_container() {
    for (suffix, kind, compression) in [
        (".tar", ArchiveType::Tar, CompressionType::None),
        (".tar.gz", ArchiveType::Tar, CompressionType::GZip),
    ] {
        let f = Fixture::new();
        let path = f.write(
            &format!("a{suffix}"),
            kind,
            compression,
            &[("keep.txt", b"k"), ("drop.txt", b"d")],
        );

        let outcome = f.execute(&f.plan(&path, &[ArchiveEdit::remove("drop.txt")]));

        assert_eq!(None, outcome.failure, "{suffix}");
        assert_eq!(vec!["keep.txt"], f.entries_of(&path), "{suffix}");
        // Still the format it started as: a rewrite must not quietly change what the file is.
        assert!(
            f.reader.read(&path, None, &CancelToken::none()).ok(),
            "{suffix}"
        );
    }
}

// --- what it refuses, and why ---

#[test]
fn a7z_is_refused_by_name_rather_than_rewritten_as_something_else() {
    let f = Fixture::new();
    let path =
        archive_fixtures::write_to(&f.root.root.join("x.7z"), archive_fixtures::PLAIN_SEVEN_ZIP);

    let plan = f.plan(&path, &[ArchiveEdit::remove("notes.txt")]);

    let rejected = plan.rejected.expect("rejected");
    assert_eq!(ArchiveEditRejection::FormatNotWritable, rejected.reason);
    assert!(rejected.message.contains(".7z"));
}

/// A rewrite would drop the encryption, which is worse than not offering it.
#[test]
fn an_encrypted_archive_is_refused() {
    let f = Fixture::new();
    let path = archive_fixtures::write_to(
        &f.root.root.join("enc.zip"),
        archive_fixtures::ENCRYPTED_ZIP,
    );

    let plan = f.plan(&path, &[ArchiveEdit::remove("notes.txt")]);

    assert_eq!(
        ArchiveEditRejection::Encrypted,
        plan.rejected.unwrap().reason
    );
}

#[test]
fn an_archive_too_large_to_rewrite_is_refused_with_its_cost() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("x.txt", b"x")]);

    let plan = f.planner.plan(
        &f.reader.read(&zip, None, &CancelToken::none()),
        &zip,
        ArchiveEditPlanner::MAX_REWRITE_BYTES + 1,
        &[ArchiveEdit::remove("x.txt")],
    );

    let rejected = plan.rejected.expect("rejected");
    assert_eq!(ArchiveEditRejection::TooLarge, rejected.reason);
    assert!(rejected.message.contains("rewriting"));
}

#[test]
fn renaming_onto_a_taken_name_is_refused() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("a.txt", b"1"), ("b.txt", b"2")]);

    let plan = f.plan(&zip, &[ArchiveEdit::rename("a.txt", "b.txt")]);

    assert_eq!(
        ArchiveEditRejection::NameTaken,
        plan.rejected.unwrap().reason
    );
}

#[test]
fn an_illegal_name_is_refused_in_the_rename_rules_own_words() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("a.txt", b"1")]);

    let plan = f.plan(&zip, &[ArchiveEdit::rename("a.txt", "b?.txt")]);

    let rejected = plan.rejected.expect("rejected");
    assert_eq!(ArchiveEditRejection::InvalidName, rejected.reason);
    assert_eq!(RenamePattern::validate("b?.txt"), Some(rejected.message));
}

#[test]
fn an_entry_that_is_no_longer_there_is_refused() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("a.txt", b"1")]);

    assert_eq!(
        ArchiveEditRejection::EntryMissing,
        f.plan(&zip, &[ArchiveEdit::remove("gone.txt")])
            .rejected
            .unwrap()
            .reason
    );
}

// --- the safety invariants ---

/// The whole reason the new container is built first. At every moment before the swap, the
/// archive on disk is the one the user started with.
#[test]
fn a_cancelled_edit_leaves_the_original_exactly_as_it_was() {
    let f = Fixture::new();
    let zip = f.zip(
        "a.zip",
        &[("one.txt", b"1"), ("two.txt", b"2"), ("three.txt", b"3")],
    );
    let before = std::fs::read(&zip).unwrap();

    let cancel = CancelToken::new();
    cancel.cancel();

    let outcome = f.executor().execute(
        &f.plan(&zip, &[ArchiveEdit::remove("two.txt")]),
        &cancel,
        None,
        None,
    );

    assert!(outcome.cancelled);
    assert_eq!(before, std::fs::read(&zip).unwrap());
    assert!(f.leftover_rewrites().is_empty());
}

#[test]
fn nothing_is_left_beside_the_archive_after_a_successful_edit() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);

    f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));

    assert!(f.leftover_rewrites().is_empty());
}

/// The meta-test: the leftover check can actually fail.
#[test]
fn the_leftover_check_notices_a_half_written_container() {
    let f = Fixture::new();
    f.root.make(
        &format!("a.zip{}-deadbeef", ArchiveEditExecutor::REWRITE_MARKER),
        b"half",
    );
    assert!(!f.leftover_rewrites().is_empty());
}

/// The original is held rather than erased, which is what makes the edit undoable, and it is
/// still there afterwards, because only retiring the history entry ever commits it.
#[test]
fn the_replaced_archive_is_held_rather_than_erased() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);

    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));

    assert!(outcome.can_undo());
    let staged = outcome.staged_original.as_deref().expect("staged");
    assert!(exists(staged));
}

#[test]
fn undo_puts_back_the_same_bytes() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let before = std::fs::read(&zip).unwrap();

    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));
    assert_eq!(vec!["one.txt"], f.entries_of(&zip));

    assert_eq!(None, f.executor().undo(&outcome).failure);
    assert_eq!(before, std::fs::read(&zip).unwrap());
    assert_eq!(vec!["one.txt", "two.txt"], f.entries_of(&zip));
}

/// The edited container is set aside, not deleted: the only thing that makes a redo possible.
#[test]
fn undo_holds_the_edited_container_rather_than_erasing_it() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));
    let edited = std::fs::read(&zip).unwrap();

    let undo = f.executor().undo(&outcome);

    assert!(undo.can_redo());
    let staged = undo.staged_edited.as_deref().expect("staged");
    assert!(bb_paths::file_name(staged).contains(ArchiveEditExecutor::EDITED_MARKER));
    assert_eq!(edited, std::fs::read(staged).unwrap());
}

#[test]
fn redo_takes_the_edit_back_in_and_can_be_undone_again() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let before = std::fs::read(&zip).unwrap();
    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));

    let executor = f.executor();
    let redo = executor.redo(&executor.undo(&outcome));
    assert_eq!(None, redo.failure);
    assert!(redo.can_undo());
    assert_eq!(vec!["one.txt"], f.entries_of(&zip));

    assert_eq!(None, executor.undo(&redo).failure);
    assert_eq!(before, std::fs::read(&zip).unwrap());
}

/// The history can reach an edit long after it was made. A container rewritten by something else
/// since is refused rather than swapped for an older original.
#[test]
fn undo_refuses_a_container_that_changed_since_the_edit() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));
    append(&zip, b"someone else's bytes");
    let changed = std::fs::read(&zip).unwrap();

    let undo = f.executor().undo(&outcome);

    assert!(undo.failure.is_some());
    assert!(!undo.can_redo());
    assert_eq!(changed, std::fs::read(&zip).unwrap());
    assert!(exists(outcome.staged_original.as_deref().unwrap()));
}

#[test]
fn redo_refuses_a_container_that_changed_since_the_undo() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let executor = f.executor();
    let undo = executor.undo(&f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")])));
    append(&zip, b"someone else's bytes");
    let changed = std::fs::read(&zip).unwrap();

    let redo = executor.redo(&undo);

    assert!(redo.failure.is_some());
    assert_eq!(changed, std::fs::read(&zip).unwrap());
    assert!(exists(undo.staged_edited.as_deref().unwrap()));
}

#[test]
fn committing_an_undo_erases_only_the_held_edited_container() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let undo = f
        .executor()
        .undo(&f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")])));
    let innocent = f.zip("precious.zip", &[("mine.txt", b"do not delete")]);

    let mut pointed_elsewhere = undo.clone();
    pointed_elsewhere.staged_edited = Some(innocent.clone());
    ArchiveEditExecutor::commit_undo_staging(&pointed_elsewhere);
    assert!(exists(&innocent));

    ArchiveEditExecutor::commit_undo_staging(&undo);
    assert!(!exists(undo.staged_edited.as_deref().unwrap()));
    assert!(exists(&zip));
}

/// Committing is what finally erases the held original, and it is the only thing that does, so
/// the replaced container outlives its undo record by exactly one operation.
#[test]
fn committing_erases_the_held_original() {
    let f = Fixture::new();
    let zip = f.zip("a.zip", &[("one.txt", b"1"), ("two.txt", b"2")]);
    let outcome = f.execute(&f.plan(&zip, &[ArchiveEdit::remove("two.txt")]));

    ArchiveEditExecutor::commit_staging(&outcome);

    assert!(!exists(outcome.staged_original.as_deref().unwrap()));
    assert!(exists(&zip));
}

/// The guard on the only erase in this file. Hand it something not named the way this type names
/// staged originals and it must do nothing: mutate the check to return true and this deletes a
/// real archive.
#[test]
fn committing_refuses_anything_that_is_not_a_held_original() {
    let f = Fixture::new();
    let innocent = f.zip("precious.zip", &[("mine.txt", b"do not delete")]);

    ArchiveEditExecutor::commit_staging(&ArchiveEditOutcome::new(
        "whatever.zip",
        Some(innocent.clone()),
        1,
        None,
        false,
    ));

    assert!(exists(&innocent));
}

fn append(path: &str, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(bytes).unwrap();
}
