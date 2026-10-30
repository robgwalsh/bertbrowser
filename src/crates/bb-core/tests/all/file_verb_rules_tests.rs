// Port of tests/BertBrowser.Core.Tests/FileVerbRulesTests.cs.
use std::cell::Cell;

use bb_core::commands::{FileVerb, FileVerbRules, FileVerbSnapshot};

/// A selection of `files` files and `folders` folders, in an ordinary folder unless said otherwise.
struct Tab {
    files: usize,
    folders: usize,
    in_archive: bool,
    search: bool,
    has_folder: bool,
    comparing: bool,
    single_archive: bool,
    all_bookmarked: bool,
    locked: bool,
    clipboard: bool,
    elevatable: bool,
}

impl Default for Tab {
    fn default() -> Self {
        Tab {
            files: 0,
            folders: 0,
            in_archive: false,
            search: false,
            has_folder: true,
            comparing: false,
            single_archive: false,
            all_bookmarked: false,
            locked: false,
            clipboard: false,
            elevatable: false,
        }
    }
}

fn tab() -> Tab {
    Tab::default()
}

impl Tab {
    fn files(mut self, n: usize) -> Tab {
        self.files = n;
        self
    }
    fn folders(mut self, n: usize) -> Tab {
        self.folders = n;
        self
    }
    fn in_archive(mut self) -> Tab {
        self.in_archive = true;
        self
    }
    fn search(mut self) -> Tab {
        self.search = true;
        self
    }
    fn no_folder(mut self) -> Tab {
        self.has_folder = false;
        self
    }
    fn comparing(mut self) -> Tab {
        self.comparing = true;
        self
    }
    fn single_archive(mut self) -> Tab {
        self.single_archive = true;
        self
    }
    fn all_bookmarked(mut self) -> Tab {
        self.all_bookmarked = true;
        self
    }
    fn locked(mut self) -> Tab {
        self.locked = true;
        self
    }
    fn clipboard(mut self) -> Tab {
        self.clipboard = true;
        self
    }
    fn elevatable(mut self) -> Tab {
        self.elevatable = true;
        self
    }

    fn on(&self, verb: FileVerb) -> bool {
        self.state(verb).enabled
    }

    fn header(&self, verb: FileVerb) -> String {
        self.state(verb).header
    }

    fn state(&self, verb: FileVerb) -> bb_core::commands::FileVerbState {
        let clipboard = self.clipboard;
        let elevatable = self.elevatable;
        let clip = move || clipboard;
        let elev = move || elevatable;
        let snapshot = FileVerbSnapshot {
            count: self.files + self.folders,
            files: self.files,
            in_archive: self.in_archive,
            is_search_result: self.search,
            has_folder: self.has_folder,
            comparing: self.comparing,
            single_is_archive: self.single_archive,
            all_bookmarked: self.all_bookmarked,
            archive_locked: self.locked,
            clipboard_has_files: Some(&clip),
            can_run_elevated: Some(&elev),
        };
        FileVerbRules::for_verb(verb, &snapshot)
    }
}

#[test]
fn every_verb_has_a_rule_and_an_unavailable_one_says_why() {
    let nothing = tab().no_folder();
    for verb in FileVerb::ALL {
        let state = nothing.state(verb);
        assert!(!state.header.trim().is_empty(), "{verb:?}");
        assert_eq!(state.enabled, state.reason.is_none(), "{verb:?}");
    }
}

// --- Inside an archive ---

#[test]
fn nothing_that_needs_a_real_path_runs_inside_an_archive() {
    for verb in [
        FileVerb::Cut,
        FileVerb::Copy,
        FileVerb::Paste,
        FileVerb::New,
        FileVerb::Compress,
        FileVerb::Checksum,
        FileVerb::VerifyChecksums,
        FileVerb::Duplicates,
        FileVerb::Changes,
        FileVerb::Bookmark,
        FileVerb::DeletePermanently,
        FileVerb::OpenInTerminal,
        FileVerb::OpenInVSCode,
    ] {
        assert!(
            !tab().files(1).in_archive().clipboard().on(verb),
            "{verb:?}"
        );
        assert!(!tab().in_archive().clipboard().on(verb), "{verb:?}");
    }
}

#[test]
fn two_files_are_not_comparable_inside_an_archive() {
    assert!(tab().files(2).on(FileVerb::CompareFiles));
    assert!(!tab().files(2).in_archive().on(FileVerb::CompareFiles));
}

#[test]
fn disk_usage_stays_on_inside_an_archive_because_every_size_there_is_exact() {
    assert!(tab().in_archive().on(FileVerb::DiskUsage));
}

#[test]
fn rename_and_delete_stay_on_inside_an_archive_but_rename_takes_one_at_a_time() {
    assert!(tab().files(1).in_archive().on(FileVerb::Rename));
    assert!(tab().files(3).in_archive().on(FileVerb::Delete));
    assert!(!tab().files(2).in_archive().on(FileVerb::Rename));
    assert!(tab().files(2).on(FileVerb::Rename));
}

#[test]
fn extract_is_the_one_write_verb_more_available_inside_than_outside() {
    let inside = tab().in_archive().state(FileVerb::ExtractHere);
    let on_archive = tab().files(1).single_archive().state(FileVerb::ExtractTo);
    let on_text = tab().files(1).state(FileVerb::ExtractHere);
    assert!(inside.visible && inside.enabled);
    assert!(on_archive.visible && on_archive.enabled);
    assert!(!on_text.visible && !on_text.enabled);
    assert_eq!(
        "Extract 3 item(s) here",
        tab().files(3).in_archive().header(FileVerb::ExtractHere)
    );
    assert_eq!(
        "Extract here",
        tab().in_archive().header(FileVerb::ExtractHere)
    );
}

// --- Search results and flat views ---

#[test]
fn a_search_result_has_no_folder_to_create_in_or_compress() {
    assert!(!tab().search().on(FileVerb::New));
    assert!(!tab().files(1).search().on(FileVerb::Compress));
    assert!(!tab().no_folder().on(FileVerb::New));
    assert!(tab().on(FileVerb::New));
    assert!(tab().on(FileVerb::Compress));
}

// --- Selection shape ---

#[test]
fn verbs_about_the_selection_need_one() {
    for verb in [
        FileVerb::Open,
        FileVerb::Cut,
        FileVerb::Copy,
        FileVerb::CopyPath,
        FileVerb::CopyName,
        FileVerb::Rename,
        FileVerb::Delete,
        FileVerb::DeletePermanently,
        FileVerb::Bookmark,
        FileVerb::Properties,
        FileVerb::Checksum,
    ] {
        assert!(!tab().on(verb), "{verb:?}");
        assert!(tab().files(1).on(verb), "{verb:?}");
    }
}

#[test]
fn a_checksum_is_of_files_only() {
    assert!(tab().files(3).on(FileVerb::Checksum));
    assert!(!tab().files(3).folders(1).on(FileVerb::Checksum));
    assert!(!tab().folders(1).on(FileVerb::Checksum));
    assert_eq!(
        "Checksums of 3 files\u{2026}",
        tab().files(3).header(FileVerb::Checksum)
    );
    assert_eq!(
        "Checksum\u{2026}",
        tab().files(1).header(FileVerb::Checksum)
    );
}

#[test]
fn comparing_files_takes_exactly_two_files() {
    for (files, folders, expected) in [
        (2, 0, true),
        (1, 0, false),
        (3, 0, false),
        (1, 1, false),
        (0, 2, false),
    ] {
        assert_eq!(
            expected,
            tab()
                .files(files)
                .folders(folders)
                .on(FileVerb::CompareFiles),
            "{files}/{folders}"
        );
    }
}

#[test]
fn opening_in_a_tab_or_pane_needs_a_folder_in_the_selection() {
    assert!(!tab().files(2).on(FileVerb::OpenInNewTab));
    assert!(tab().files(2).folders(1).on(FileVerb::OpenInNewTab));
    assert!(tab().folders(1).on(FileVerb::OpenInNewPane));
    assert_eq!(
        "Open 3 folders in new tabs",
        tab().folders(3).header(FileVerb::OpenInNewTab)
    );
    assert_eq!(
        "Open in new tab",
        tab().folders(1).header(FileVerb::OpenInNewTab)
    );
}

#[test]
fn folder_tools_take_one_folder_or_the_folder_being_shown() {
    for verb in [FileVerb::DiskUsage, FileVerb::Duplicates, FileVerb::Changes] {
        assert!(tab().on(verb), "{verb:?}");
        assert!(tab().folders(1).on(verb), "{verb:?}");
        assert!(!tab().files(1).on(verb), "{verb:?}");
        assert!(!tab().folders(2).on(verb), "{verb:?}");
    }
}

#[test]
fn terminal_and_editor_fall_back_to_the_folder_being_shown() {
    assert!(tab().on(FileVerb::OpenInTerminal));
    assert!(tab().files(1).no_folder().on(FileVerb::OpenInVSCode));
    assert!(!tab().no_folder().on(FileVerb::OpenInTerminal));
}

#[test]
fn run_as_administrator_is_for_one_item_with_something_to_elevate() {
    assert!(tab().files(1).elevatable().on(FileVerb::RunAsAdmin));
    assert!(!tab().files(1).on(FileVerb::RunAsAdmin));
    assert!(!tab().files(2).elevatable().on(FileVerb::RunAsAdmin));
    assert!(!tab().elevatable().on(FileVerb::RunAsAdmin));
}

#[test]
fn the_registry_is_only_asked_about_a_single_item() {
    let asked = Cell::new(0);
    let probe = || {
        asked.set(asked.get() + 1);
        true
    };
    let mut two = FileVerbSnapshot::new(2, 2, false, false, true);
    two.can_run_elevated = Some(&probe);
    FileVerbRules::for_verb(FileVerb::RunAsAdmin, &two);
    FileVerbRules::for_verb(FileVerb::Rename, &two);
    assert_eq!(0, asked.get());
}

#[test]
fn the_clipboard_is_only_asked_by_paste() {
    let asked = Cell::new(0);
    let probe = || {
        asked.set(asked.get() + 1);
        true
    };
    let mut snapshot = FileVerbSnapshot::new(1, 1, false, false, true);
    snapshot.clipboard_has_files = Some(&probe);
    for verb in FileVerb::ALL.iter().filter(|v| **v != FileVerb::Paste) {
        FileVerbRules::for_verb(*verb, &snapshot);
    }
    assert_eq!(0, asked.get());
    assert!(FileVerbRules::for_verb(FileVerb::Paste, &snapshot).enabled);
    assert_eq!(1, asked.get());
}

// --- Comparison ---

#[test]
fn settling_by_content_exists_only_while_comparing() {
    let idle = tab().files(1).state(FileVerb::SettleByContent);
    let comparing = tab().files(2).comparing().state(FileVerb::SettleByContent);
    let folders_only = tab()
        .folders(1)
        .comparing()
        .state(FileVerb::SettleByContent);
    assert!(!idle.visible && !idle.enabled);
    assert!(
        comparing.visible && comparing.enabled && comparing.header == "Settle these by content"
    );
    assert!(folders_only.visible && !folders_only.enabled);
}

#[test]
fn compare_is_never_off_and_says_what_pressing_it_will_do() {
    assert_eq!(
        "Compare with other pane",
        tab().header(FileVerb::ComparePanes)
    );
    assert_eq!(
        "Stop comparing",
        tab().comparing().header(FileVerb::ComparePanes)
    );
    assert!(tab().in_archive().no_folder().on(FileVerb::ComparePanes));
}

// --- Wording ---

#[test]
fn headers_count_what_they_will_act_on() {
    assert_eq!(
        "Rename 4 items\u{2026}",
        tab().files(4).header(FileVerb::Rename)
    );
    assert_eq!("Rename\u{2026}", tab().files(1).header(FileVerb::Rename));
    assert_eq!(
        "Delete 2 items\u{2026}",
        tab().files(2).header(FileVerb::Delete)
    );
    assert_eq!("Copy as paths", tab().files(2).header(FileVerb::CopyPath));
    assert_eq!("Copy names", tab().files(2).header(FileVerb::CopyName));
    assert_eq!(
        "Compress 1,200 items\u{2026}",
        tab().files(1200).header(FileVerb::Compress)
    );
}

#[test]
fn bookmark_becomes_remove_only_when_everything_selected_is_bookmarked() {
    assert_eq!("Bookmark", tab().files(2).header(FileVerb::Bookmark));
    assert_eq!(
        "Remove bookmark",
        tab().files(2).all_bookmarked().header(FileVerb::Bookmark)
    );
}

#[test]
fn unlock_is_for_a_locked_archive() {
    assert!(tab().in_archive().locked().on(FileVerb::UnlockArchive));
    assert!(!tab().in_archive().on(FileVerb::UnlockArchive));
}
