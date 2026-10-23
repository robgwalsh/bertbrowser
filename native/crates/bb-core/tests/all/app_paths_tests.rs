// Not a port: AppPaths has no C# tests. Pins where the data directory is and what moves it.
use std::path::Path;

use bb_core::settings::{AppPaths, LegacyData};

#[test]
fn the_default_is_dot_bertbrowser_under_the_profile() {
    // Not %LOCALAPPDATA%\BertBrowser: Velopack installs there and uninstall deletes it.
    let paths = AppPaths::resolve(None, Path::new(r"C:\Users\Rob"));

    assert_eq!(Path::new(r"C:\Users\Rob\.bertbrowser"), paths.data_dir());
    assert_eq!(
        Path::new(r"C:\Users\Rob\.bertbrowser\bertbrowser.db"),
        paths.db_path()
    );
    assert_eq!(
        Path::new(r"C:\Users\Rob\.bertbrowser\settings.json"),
        paths.settings_path()
    );
    assert_eq!(
        Path::new(r"C:\Users\Rob\.bertbrowser\themes"),
        paths.themes_dir()
    );
    assert_eq!(
        Path::new(r"C:\Users\Rob\.bertbrowser\templates"),
        paths.templates_dir()
    );
    assert_eq!(
        Path::new(r"C:\Users\Rob\.bertbrowser").join(LegacyData::MARKER_FILE_NAME),
        paths.native_marker_path()
    );
}

#[test]
fn the_override_variable_moves_everything() {
    let paths = AppPaths::resolve(Some(r"D:\scratch\bb"), Path::new(r"C:\Users\Rob"));

    assert_eq!(Path::new(r"D:\scratch\bb"), paths.data_dir());
    assert_eq!(
        Path::new(r"D:\scratch\bb\settings.json"),
        paths.settings_path()
    );
    assert_eq!(Path::new(r"D:\scratch\bb\templates"), paths.templates_dir());
}

#[test]
fn an_empty_override_is_not_set() {
    let paths = AppPaths::resolve(Some(""), Path::new(r"C:\Users\Rob"));

    assert_eq!(Path::new(r"C:\Users\Rob\.bertbrowser"), paths.data_dir());
}

#[test]
fn the_override_is_made_absolute() {
    let paths = AppPaths::resolve(Some(r"D:\a\..\b\"), Path::new(r"C:\Users\Rob"));

    assert_eq!(Path::new(r"D:\b"), paths.data_dir());
}

#[test]
fn the_variable_name_is_the_one_the_harness_sets() {
    assert_eq!("BERTBROWSER_DATA_DIR", AppPaths::OVERRIDE_VARIABLE);
}
