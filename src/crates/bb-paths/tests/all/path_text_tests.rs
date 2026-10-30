// Port of tests/BertBrowser.Core.Tests/PathTextTests.cs.
use bb_paths::PathText;

/// Explorer quotes every path, space or no space, and so does this: a result that is only sometimes
/// paste-safe is worse than one that always is.
#[test]
fn every_path_is_quoted() {
    for path in [r"C:\Program Files\app.exe", r"C:\tools\app.exe"] {
        assert_eq!(format!("\"{path}\""), PathText::quote(path));
    }
}

#[test]
fn several_paths_are_one_per_line() {
    let text = PathText::for_clipboard([r"C:\a.txt", r"C:\b b.txt"]);
    assert_eq!("\"C:\\a.txt\"\r\n\"C:\\b b.txt\"", text);
}

#[test]
fn names_are_bare() {
    let text = PathText::names_for_clipboard([r"C:\dir\a.txt", r"C:\dir\sub"]);
    assert_eq!("a.txt\r\nsub", text);
}

#[test]
fn nothing_selected_is_empty_rather_than_a_stray_newline() {
    assert_eq!("", PathText::for_clipboard([]));
    assert_eq!("", PathText::names_for_clipboard([]));
}
