// Port of tests/BertBrowser.Core.Tests/RenamePatternTests.cs. The naming rule the rename dialog
// previews and the rename itself obeys: one item takes the typed name whole, several are numbered
// while keeping their own extensions.
use bb_core::rename::{RenamePattern, RenameSource};

fn file(path: &str) -> RenameSource {
    RenameSource::file(path)
}

fn dir(path: &str) -> RenameSource {
    RenameSource::dir(path)
}

// --- one item takes the name as typed ---

#[test]
fn one_item_takes_the_pattern_as_its_whole_name() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\old.txt")], "notes.txt");
    assert_eq!(vec!["notes.txt"], names);
}

#[test]
fn one_item_keeps_whatever_extension_was_typed_including_none() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\old.txt")], "notes");
    assert_eq!(vec!["notes"], names);
}

#[test]
fn surrounding_space_is_dropped() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\old.txt")], "  notes.txt  ");
    assert_eq!(vec!["notes.txt"], names);
}

#[test]
fn trailing_dots_are_dropped_because_windows_drops_them_anyway() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\old.txt")], "notes.");
    assert_eq!(vec!["notes"], names);
}

// --- several items are numbered ---

#[test]
fn several_items_are_numbered_from_one_and_keep_their_own_extensions() {
    let names = RenamePattern::apply_simple(
        &[
            file(r"C:\p\a.jpg"),
            file(r"C:\p\b.png"),
            file(r"C:\p\c.jpg"),
        ],
        "Holiday",
    );
    assert_eq!(
        vec!["Holiday 1.jpg", "Holiday 2.png", "Holiday 3.jpg"],
        names
    );
}

#[test]
fn several_items_number_in_the_order_given() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\z.txt"), file(r"C:\p\a.txt")], "Note");
    assert_eq!(vec!["Note 1.txt", "Note 2.txt"], names);
}

#[test]
fn a_folder_in_a_batch_gets_no_extension() {
    // "My.Project" is a folder, so ".Project" is part of its name, not an extension to keep.
    let names = RenamePattern::apply_simple(&[dir(r"C:\p\My.Project"), dir(r"C:\p\Other")], "Work");
    assert_eq!(vec!["Work 1", "Work 2"], names);
}

#[test]
fn a_file_with_no_extension_is_just_numbered() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\LICENSE"), file(r"C:\p\a.txt")], "Doc");
    assert_eq!(vec!["Doc 1", "Doc 2.txt"], names);
}

#[test]
fn no_items_produces_no_names() {
    assert!(RenamePattern::apply_simple(&[], "anything").is_empty());
}

// --- what the dialog starts with ---

#[test]
fn suggestion_for_one_file_is_its_whole_name() {
    assert_eq!(
        "report.pdf",
        RenamePattern::suggest_for(&[file(r"C:\p\report.pdf")])
    );
}

#[test]
fn suggestion_for_several_files_drops_the_extension_because_numbering_readds_it() {
    assert_eq!(
        "report",
        RenamePattern::suggest_for(&[file(r"C:\p\report.pdf"), file(r"C:\p\other.pdf")])
    );
}

#[test]
fn suggestion_for_several_folders_keeps_the_whole_name() {
    assert_eq!(
        "My.Project",
        RenamePattern::suggest_for(&[dir(r"C:\p\My.Project"), dir(r"C:\p\Other")])
    );
}

#[test]
fn base_name_length_covers_the_stem_only() {
    assert_eq!(
        6,
        RenamePattern::base_name_length(&file(r"C:\p\report.pdf"))
    );
}

#[test]
fn base_name_length_of_a_dotfile_covers_the_whole_name() {
    assert_eq!(
        10,
        RenamePattern::base_name_length(&file(r"C:\p\.gitignore"))
    );
}

#[test]
fn base_name_length_of_a_folder_covers_the_whole_name() {
    assert_eq!(
        10,
        RenamePattern::base_name_length(&dir(r"C:\p\My.Project"))
    );
}

// --- validation ---

#[test]
fn legal_names_validate() {
    // Only the exact device name is reserved, so "CONTENTS.txt" is fine.
    for name in [
        "notes.txt",
        ".gitignore",
        "a name with spaces.md",
        "CONTENTS.txt",
    ] {
        assert_eq!(None, RenamePattern::validate(name), "{name}");
    }
}

#[test]
fn illegal_names_are_rejected_with_a_reason() {
    for name in [
        "",
        "   ",
        "a/b.txt",
        r"a\b.txt",
        "a:b.txt",
        "a*b.txt",
        "a?b.txt",
        "a\"b.txt",
        "a<b.txt",
        "a|b.txt",
        "trailing.",
        "trailing ",
        "CON",
        "con.txt",
        "LPT1.log",
    ] {
        let problem = RenamePattern::validate(name);
        assert!(
            problem.is_some_and(|p| !p.trim().is_empty()),
            "{name:?} should be refused"
        );
    }
}

#[test]
fn overlong_names_are_rejected() {
    assert!(RenamePattern::validate(&"a".repeat(RenamePattern::MAX_NAME_LENGTH + 1)).is_some());
}

#[test]
fn a_name_at_the_limit_is_accepted() {
    assert_eq!(
        None,
        RenamePattern::validate(&"a".repeat(RenamePattern::MAX_NAME_LENGTH))
    );
}

// --- the plain box is unchanged, including where it was never covered ---
//
// These are the cases the advanced-rename engine could have broken silently: every multi-item
// test above uses an already-clean pattern, and none of them types a brace.

#[test]
fn a_dirty_pattern_is_cleaned_before_numbering_not_after() {
    // Cleaning the finished name instead would give "Holiday   1.jpg": the trim cannot reach
    // the middle, because by then the name ends in ".jpg".
    let names =
        RenamePattern::apply_simple(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.jpg")], "  Holiday  ");
    assert_eq!(vec!["Holiday 1.jpg", "Holiday 2.jpg"], names);
}

#[test]
fn a_trailing_dot_is_dropped_before_numbering() {
    let names = RenamePattern::apply_simple(&[file(r"C:\p\a.txt"), file(r"C:\p\b.txt")], "notes.");
    assert_eq!(vec!["notes 1.txt", "notes 2.txt"], names);
}

#[test]
fn braces_are_taken_literally_because_they_are_legal_in_a_name() {
    // "{6B99A0C1}.tmp" is an ordinary name to give a file and the plain box has always
    // accepted it. Tokens belong to the expanded panel and nowhere else.
    let names = RenamePattern::apply_simple(&[file(r"C:\p\old.tmp")], "{6B99A0C1}.tmp");
    assert_eq!(vec!["{6B99A0C1}.tmp"], names);
}

#[test]
fn an_empty_pattern_over_several_items_produces_names_that_are_refused() {
    // Numbering an empty pattern gives " 1.jpg", which Validate accepts — so a box the user
    // had merely cleared would leave Rename enabled.
    let names = RenamePattern::apply_simple(&[file(r"C:\p\a.jpg"), file(r"C:\p\b.jpg")], "");
    for name in &names {
        assert!(RenamePattern::validate(name).is_some(), "{name:?}");
    }
}

// --- the stem/extension split the scopes and case transforms are built on ---

#[test]
fn split_of_a_file_separates_the_extension() {
    assert_eq!(
        ("report".to_string(), ".pdf".to_string()),
        RenamePattern::split(&file(r"C:\p\report.pdf"))
    );
}

#[test]
fn split_of_a_folder_keeps_the_whole_name_because_a_folder_has_no_extension() {
    assert_eq!(
        ("My.Project".to_string(), String::new()),
        RenamePattern::split(&dir(r"C:\p\My.Project"))
    );
}

#[test]
fn split_of_a_dotfile_keeps_the_whole_name() {
    // Path calls ".gitignore" all extension and no stem. Taking that literally would leave a
    // stem-scoped find/replace nothing to work on, and let an extension-scoped one rewrite
    // the entire filename.
    assert_eq!(
        (".gitignore".to_string(), String::new()),
        RenamePattern::split(&file(r"C:\p\.gitignore"))
    );
}
