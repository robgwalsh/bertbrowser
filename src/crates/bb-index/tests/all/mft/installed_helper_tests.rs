// The administrator-only copy of the helper: where it goes, when it is copied, and the same-build
// gate. Real files under %TEMP%; the Program Files folder itself is never touched.
use std::path::{Path, PathBuf};

use bb_index::mft::{
    APP_FILE_NAME, AppDirectoryFault, HELPER_FILE_NAME, INSTALL_FOLDER_NAME, InstalledHelper,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("bb-installed-helper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn file(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_installed_path_is_under_program_files() {
    let path = InstalledHelper::path_under(Path::new(r"C:\Program Files"));
    assert_eq!(
        PathBuf::from(r"C:\Program Files")
            .join(INSTALL_FOLDER_NAME)
            .join(HELPER_FILE_NAME),
        path
    );
    assert_eq!("bb-indexer.exe", HELPER_FILE_NAME);
}

#[test]
fn program_files_comes_from_the_shell() {
    let pf = InstalledHelper::program_files_directory().expect("a program files folder");
    assert!(pf.is_absolute() && pf.is_dir(), "{pf:?}");
}

#[test]
fn identical_files_are_identical_and_nothing_else_is() {
    let s = Scratch::new("ident");
    let a = s.file("a", &[7u8; 200_000]);
    let b = s.file("b", &[7u8; 200_000]);
    let mut other = vec![7u8; 200_000];
    other[150_000] = 8;
    let c = s.file("c", &other);
    let d = s.file("d", &[7u8; 10]);

    assert!(InstalledHelper::files_are_identical(&a, &b));
    assert!(!InstalledHelper::files_are_identical(&a, &c));
    assert!(!InstalledHelper::files_are_identical(&a, &d));
    assert!(!InstalledHelper::files_are_identical(
        &a,
        &s.0.join("missing")
    ));
}

#[test]
fn install_creates_the_folder_and_copies() {
    let s = Scratch::new("install");
    let source = s.file("app/bb-indexer.exe", b"build one");
    let folder = s.0.join("pf").join("BertBrowser");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();

    let target = InstalledHelper::install(&source, &folder).unwrap();

    assert_eq!(folder.join(HELPER_FILE_NAME), target);
    assert_eq!(b"build one".to_vec(), std::fs::read(&target).unwrap());
}

#[test]
fn install_replaces_an_older_copy_and_leaves_nothing_behind() {
    let s = Scratch::new("replace");
    let source = s.file("app/bb-indexer.exe", b"build two");
    let folder = s.0.join("pf");
    s.file("pf/bb-indexer.exe", b"build one");

    let target = InstalledHelper::install(&source, &folder).unwrap();

    assert_eq!(b"build two".to_vec(), std::fs::read(&target).unwrap());
    let names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(vec![HELPER_FILE_NAME.to_string()], names);
}

#[test]
fn install_skips_the_copy_when_the_bytes_already_match() {
    let s = Scratch::new("skip");
    let source = s.file("app/bb-indexer.exe", b"same");
    let folder = s.0.join("pf");
    let existing = s.file("pf/bb-indexer.exe", b"same");
    let before = std::fs::metadata(&existing).unwrap().modified().unwrap();

    InstalledHelper::install(&source, &folder).unwrap();

    assert_eq!(
        before,
        std::fs::metadata(&existing).unwrap().modified().unwrap()
    );
}

#[test]
fn install_refuses_a_folder_that_is_a_junction() {
    let s = Scratch::new("junction");
    let source = s.file("app/bb-indexer.exe", b"x");
    let real = s.0.join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = s.0.join("link");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .output()
        .unwrap();
    assert!(made.status.success(), "mklink failed");

    let error = InstalledHelper::install(&source, &link).unwrap_err();

    assert!(error.contains("link"), "{error}");
    assert!(!real.join(HELPER_FILE_NAME).exists());
    let _ = std::fs::remove_dir(&link);
}

#[test]
fn remove_deletes_the_copy_and_the_old_one_and_the_empty_folder() {
    let s = Scratch::new("remove");
    let folder = s.0.join("pf");
    s.file("pf/bb-indexer.exe", b"x");
    s.file("pf/bb-indexer.exe.old", b"y");

    InstalledHelper::remove(&folder);

    assert!(!folder.exists());
}

#[test]
fn remove_keeps_a_folder_that_holds_something_else() {
    let s = Scratch::new("keep");
    let folder = s.0.join("pf");
    s.file("pf/bb-indexer.exe", b"x");
    let other = s.file("pf/other.txt", b"z");

    InstalledHelper::remove(&folder);

    assert!(other.exists());
    assert!(!folder.join(HELPER_FILE_NAME).exists());
}

#[test]
fn the_gate_passes_for_an_identical_helper_in_the_app_folder() {
    let s = Scratch::new("gate-ok");
    let own = s.file("pf/bb-indexer.exe", b"build one");
    s.file("app/bb-indexer.exe", b"build one");
    s.file(&format!("app/{APP_FILE_NAME}"), b"app");

    assert_eq!(
        Ok(()),
        InstalledHelper::check_app_directory(&own, &s.0.join("app"))
    );
}

#[test]
fn the_gate_refuses_a_stale_or_missing_helper_in_the_app_folder() {
    let s = Scratch::new("gate-stale");
    let own = s.file("pf/bb-indexer.exe", b"build one");
    s.file(&format!("app/{APP_FILE_NAME}"), b"app");
    let app = s.0.join("app");

    // No helper there at all.
    assert_eq!(
        Err(AppDirectoryFault::HelperDiffers),
        InstalledHelper::check_app_directory(&own, &app)
    );
    // An updated one.
    s.file("app/bb-indexer.exe", b"build two");
    assert_eq!(
        Err(AppDirectoryFault::HelperDiffers),
        InstalledHelper::check_app_directory(&own, &app)
    );
}

#[test]
fn the_gate_refuses_when_the_app_is_gone() {
    let s = Scratch::new("gate-gone");
    let own = s.file("app/bb-indexer.exe", b"x");

    assert_eq!(
        Err(AppDirectoryFault::AppMissing),
        InstalledHelper::check_app_directory(&own, &s.0.join("app"))
    );
}

/// Running from beside the app needs no comparison with itself.
#[test]
fn the_gate_needs_no_copy_when_running_beside_the_app() {
    let s = Scratch::new("gate-beside");
    let own = s.file("app/bb-indexer.exe", b"x");
    s.file(&format!("app/{APP_FILE_NAME}"), b"app");

    assert_eq!(
        Ok(()),
        InstalledHelper::check_app_directory(&own, &s.0.join("app"))
    );
}
