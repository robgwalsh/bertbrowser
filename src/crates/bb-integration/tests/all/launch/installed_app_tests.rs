use bb_integration::launch::InstalledApp;

const LOCAL: &str = r"C:\Users\u\AppData\Local";
const DEFAULT: &str = r"C:\Users\u\AppData\Local\BertCut\current\BertCut.exe";

fn only<'a>(present: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
    move |path: &str| present.iter().any(|p| p.eq_ignore_ascii_case(path))
}

#[test]
fn the_default_location_is_found_without_the_registry() {
    assert_eq!(
        Some(DEFAULT.to_string()),
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some(LOCAL),
            None,
            only(&[DEFAULT])
        )
    );
}

/// It is the app under `current`, never the stub beside it: the stub applies updates and then
/// starts the app without waiting, so a file handed to it would arrive late or not at all.
#[test]
fn the_stub_beside_current_is_not_the_app() {
    assert_eq!(
        None,
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some(LOCAL),
            None,
            only(&[r"C:\Users\u\AppData\Local\BertCut\BertCut.exe"])
        )
    );
}

/// `winget install --location` puts it elsewhere, and only the Uninstall key says where.
#[test]
fn a_relocated_install_is_found_through_its_install_location() {
    let moved = r"D:\Apps\BertCut\current\BertCut.exe";
    assert_eq!(
        Some(moved.to_string()),
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some(LOCAL),
            Some(r"D:\Apps\BertCut\"),
            only(&[moved])
        )
    );
}

#[test]
fn the_default_location_wins_when_both_are_there() {
    assert_eq!(
        Some(DEFAULT.to_string()),
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some(LOCAL),
            Some(r"D:\Apps\BertCut"),
            only(&[DEFAULT, r"D:\Apps\BertCut\current\BertCut.exe"])
        )
    );
}

#[test]
fn nothing_installed_is_none() {
    assert_eq!(
        None,
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some(LOCAL),
            Some(r"D:\Apps\BertCut"),
            only(&[])
        )
    );
    assert_eq!(
        None,
        InstalledApp::locate("BertCut", "BertCut.exe", None, None, |_| true)
    );
}

/// A relative root would resolve against wherever this process was started, and that must not be
/// what picks a program to run.
#[test]
fn a_root_that_is_not_a_full_path_is_ignored() {
    assert_eq!(
        None,
        InstalledApp::locate(
            "BertCut",
            "BertCut.exe",
            Some("Local"),
            Some(r"..\x"),
            |_| true
        )
    );
}

#[test]
fn a_name_that_could_climb_out_of_its_folder_is_refused() {
    for (pack, exe) in [
        (r"..\..\Windows", "BertCut.exe"),
        ("BertCut", r"..\evil.exe"),
        ("", "BertCut.exe"),
        ("BertCut", ""),
        ("..", "BertCut.exe"),
    ] {
        assert_eq!(
            None,
            InstalledApp::locate(pack, exe, Some(LOCAL), None, |_| true),
            "{pack} / {exe}"
        );
    }
}
