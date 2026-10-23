// Port of tests/BertBrowser.Core.Tests/VSCodePathTests.cs.
//
// The rule that keeps a console window off the screen. `code` on PATH is a batch shim, and
// starting a batch file through the shell starts cmd.exe to do it — so the editor arrives with a
// terminal beside it. Stepping over the shim is the fix, and the danger in stepping over one is
// starting the *wrong* program, which is why nearly every test here asserts a refusal.
use bb_core::launch::VsCodePath;

/// A filesystem containing exactly what it is told to, answering the way `ExecutablePath::resolve`
/// does over a fully qualified path.
fn existing(paths: &[&str]) -> impl Fn(&str) -> Option<String> {
    let set: Vec<String> = paths.iter().map(|p| p.to_lowercase()).collect();
    move |candidate: &str| {
        set.contains(&candidate.to_lowercase())
            .then(|| candidate.to_string())
    }
}

fn assert_path(expected: &str, actual: Option<String>) {
    assert_eq!(
        Some(expected.to_lowercase()),
        actual.map(|a| a.to_lowercase())
    );
}

const BIN: &str = r"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd";
const EDITOR: &str = r"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\Code.exe";

#[test]
fn the_shim_becomes_the_editor_above_it() {
    assert_path(
        EDITOR,
        VsCodePath::behind_launcher(Some(BIN), existing(&[EDITOR])),
    );
}

/// The case the stem does not name: the launcher is `code-insiders.cmd` and the program beside it
/// is `Code - Insiders.exe`.
#[test]
fn an_insiders_shim_finds_the_name_it_does_not_share() {
    const SHIM: &str = r"C:\VS Code Insiders\bin\code-insiders.cmd";
    const EDITOR: &str = r"C:\VS Code Insiders\Code - Insiders.exe";

    assert_path(
        EDITOR,
        VsCodePath::behind_launcher(Some(SHIM), existing(&[EDITOR])),
    );
}

/// Preference order, not "the first thing found": an install holding both is the stable one with
/// an Insiders build unpacked beside it, and stable is what `code` means.
#[test]
fn stable_wins_over_insiders_in_one_root() {
    assert_path(
        EDITOR,
        VsCodePath::behind_launcher(
            Some(BIN),
            existing(&[
                EDITOR,
                r"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\Code - Insiders.exe",
            ]),
        ),
    );
}

/// Nothing recognisable above the shim is a None, never a guess — the caller then starts the shim
/// it already had, console window and all, which opens the right editor.
#[test]
fn an_unrecognised_install_is_refused() {
    assert_eq!(
        None,
        VsCodePath::behind_launcher(
            Some(BIN),
            existing(&[r"C:\Users\Rob\AppData\Local\Programs\Microsoft VS Code\unins000.exe"]),
        )
    );
}

/// The `bin` requirement is what stops this being "run the .exe next to any .cmd". A shim sitting
/// beside its own program would otherwise send us a level up into a folder full of other people's
/// programs.
#[test]
fn a_shim_outside_a_bin_folder_is_refused() {
    assert_eq!(
        None,
        VsCodePath::behind_launcher(Some(r"C:\tools\code.cmd"), existing(&[r"C:\Code.exe"]))
    );
}

#[test]
fn an_executable_is_not_a_shim_and_is_left_alone() {
    assert_eq!(
        None,
        VsCodePath::behind_launcher(Some(EDITOR), existing(&[EDITOR]))
    );
}

#[test]
fn nothing_resolved_is_nothing_to_step_over() {
    for launcher in [None, Some(""), Some("   ")] {
        assert_eq!(
            None,
            VsCodePath::behind_launcher(launcher, existing(&[EDITOR])),
            "{launcher:?}"
        );
    }
}

/// A `.bat` is the same shape and the same trap; VSCodium ships one of these layouts too.
#[test]
fn a_bat_shim_is_taken_the_same_way() {
    const SHIM: &str = r"C:\Program Files\VSCodium\bin\codium.bat";
    const EDITOR: &str = r"C:\Program Files\VSCodium\VSCodium.exe";

    assert_path(
        EDITOR,
        VsCodePath::behind_launcher(Some(SHIM), existing(&[EDITOR])),
    );
}

/// An install laid out at a drive root still resolves: the root above the `bin` is `C:\`, which
/// is a folder like any other.
#[test]
fn a_shim_at_a_drive_root_still_finds_the_editor() {
    assert_path(
        r"C:\Code.exe",
        VsCodePath::behind_launcher(Some(r"C:\bin\code.cmd"), existing(&[r"C:\Code.exe"])),
    );
}
