// Port of tests/BertBrowser.Core.Tests/ShellMenuKeysTests.cs: which registry key families are
// asked for a selection's context-menu entries. The list has to match Explorer's, or an extension
// that shows there is missing here for no reason a user could find.
use bb_core::shellmenu::{ShellFileType, ShellMenuContext, ShellMenuKeys, ShellMenuTarget};

fn no_type(_: &str) -> Option<ShellFileType> {
    None
}

fn text_type(extension: &str) -> Option<ShellFileType> {
    (extension == ".txt").then(|| ShellFileType::new(Some("txtfile"), Some("text")))
}

fn target(path: &str, is_directory: bool) -> ShellMenuTarget {
    ShellMenuTarget::new(path, is_directory)
}

#[test]
fn a_file_asks_its_prog_id_its_extension_its_perceived_type_then_everything() {
    let keys = ShellMenuKeys::for_targets(
        &[target(r"C:\docs\notes.txt", false)],
        ShellMenuContext::Items,
        text_type,
    );

    assert_eq!(
        vec![
            "txtfile",
            r"SystemFileAssociations\.txt",
            r"SystemFileAssociations\text",
            "*",
            "AllFilesystemObjects"
        ],
        keys
    );
}

#[test]
fn a_file_of_unknown_type_still_asks_the_generic_families() {
    let keys = ShellMenuKeys::for_targets(
        &[target(r"C:\docs\thing.xyz", false)],
        ShellMenuContext::Items,
        no_type,
    );

    assert_eq!(
        vec![r"SystemFileAssociations\.xyz", "*", "AllFilesystemObjects"],
        keys
    );
}

#[test]
fn a_file_with_no_extension_asks_only_the_generic_families() {
    let keys = ShellMenuKeys::for_targets(
        &[target(r"C:\docs\README", false)],
        ShellMenuContext::Items,
        no_type,
    );

    assert_eq!(vec!["*", "AllFilesystemObjects"], keys);
}

#[test]
fn a_folder_asks_directory_and_folder() {
    let keys = ShellMenuKeys::for_targets(
        &[target(r"C:\docs\photos", true)],
        ShellMenuContext::Items,
        no_type,
    );

    assert_eq!(vec!["Directory", "Folder", "AllFilesystemObjects"], keys);
}

#[test]
fn a_drive_root_asks_drive_instead_of_directory() {
    for root in [r"D:\", "D:"] {
        let keys =
            ShellMenuKeys::for_targets(&[target(root, true)], ShellMenuContext::Items, no_type);

        assert_eq!(
            vec!["Drive", "Folder", "AllFilesystemObjects"],
            keys,
            "{root}"
        );
    }
}

/// Explorer asks about the first item's type and lets the handlers see the rest through the data
/// object; a mixed selection therefore follows whatever came first.
#[test]
fn a_mixed_selection_follows_the_first_item() {
    let keys = ShellMenuKeys::for_targets(
        &[
            target(r"C:\docs\photos", true),
            target(r"C:\docs\notes.txt", false),
        ],
        ShellMenuContext::Items,
        text_type,
    );

    assert_eq!(vec!["Directory", "Folder", "AllFilesystemObjects"], keys);
}

#[test]
fn the_background_is_its_own_family_and_nothing_else() {
    let keys = ShellMenuKeys::for_targets(&[], ShellMenuContext::Background, text_type);

    assert_eq!(vec![r"Directory\Background"], keys);
}

#[test]
fn no_items_means_no_keys() {
    assert!(ShellMenuKeys::for_targets(&[], ShellMenuContext::Items, no_type).is_empty());
}
