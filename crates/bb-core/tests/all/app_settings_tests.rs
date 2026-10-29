// Not a port: AppSettings has no C# tests. Pins the defaults, the JSON shape, the round trip, and
// the one fact a load carries that the file does not — `loaded_from_disk`.
use std::collections::BTreeMap;

use bb_core::FileTime;
use bb_core::changes::ChangeLogPolicy;
use bb_core::columns::ColumnSetting;
use bb_core::layout::SessionLayout;
use bb_core::settings::{
    AppSettings, ChecksumAlgorithm, CustomCommandDefinition, DrivesOpenTarget, RenameCase,
    RenameRule, RenameScope, SectionPlacement,
};
use bb_test_support::unique_temp_path;

fn temp_settings_file() -> std::path::PathBuf {
    unique_temp_path("bertbrowser-settings", ".json")
}

#[test]
fn the_defaults_are_the_c_sharp_defaults() {
    let s = AppSettings::default();

    assert_eq!(None, s.window_left);
    assert_eq!(None, s.window_width);
    assert!(!s.window_maximized);
    assert_eq!(None, s.last_path);
    assert_eq!(None, s.session);
    assert!(s.restore_last_session);
    assert_eq!(None, s.startup_default_path);
    assert!(!s.show_hidden_items);
    assert!(s.directory_thumbnail_scales.is_empty());
    assert_eq!(2.0, s.scroll_speed_multiplier);
    assert!(!s.show_preview_pane);
    assert_eq!(360.0, s.preview_pane_width);
    assert!(!s.show_metadata_pane);
    assert!(!s.preview_auto_advance);
    assert!(!s.preview_loop);
    assert!(!s.preview_auto_play);
    assert_eq!(None, s.file_list_columns);
    assert_eq!(1024 * 1024, s.preview_text_max_bytes);
    assert_eq!(1024 * 1024, s.search_content_max_bytes);
    assert_eq!(1024 * 1024, s.duplicate_min_size_bytes);
    assert_eq!("Sha256", s.checksum_algorithms);
    assert!(!s.record_file_changes);
    assert!(!s.start_indexer_at_launch);
    assert_eq!(
        ChangeLogPolicy::DEFAULT_RETENTION_HOURS,
        s.file_change_retention_hours
    );
    assert_eq!(None, s.change_timeline_mark_utc);
    assert_eq!(None, s.undo_max_steps);
    assert_eq!(None, s.undo_max_held_gigabytes);
    assert!(s.duplicate_skip_system_folders);
    assert_eq!("4:3", s.tile_aspect_ratio);
    assert!(s.custom_commands.is_empty());
    assert_eq!(None, s.key_bindings);
    assert!(s.palette_pins.is_empty());
    assert!(s.show_shell_extensions);
    assert!(s.hidden_shell_extensions.is_empty());
    assert!(s.hidden_built_in_menu_items.is_empty());
    assert_eq!(None, s.context_menu_layout);
    assert!(s.command_bars.is_none());
    assert!(s.saved_command_bars.is_empty());
    assert_eq!(None, s.new_file_types);
    assert_eq!(None, s.theme_id);
    assert_eq!(None, s.follow_system_theme);
    assert_eq!(None, s.light_theme_id);
    assert_eq!(None, s.dark_theme_id);
    assert!(!s.loaded_from_disk());
    assert!(s.theme_overrides.is_empty());
    assert!(!s.advanced_rename_expanded);
    assert_eq!(None, s.advanced_rename);
    assert!(s.enter_archives_on_double_click);
    assert_eq!(DrivesOpenTarget::NewTab, s.drives_open_target);
    assert_eq!(SectionPlacement::Sidebar, s.workspaces_placement);
    assert_eq!(SectionPlacement::Sidebar, s.saved_searches_placement);
}

#[test]
fn a_missing_file_yields_defaults_not_loaded_from_disk() {
    let path = temp_settings_file();

    let s = AppSettings::load(&path);

    assert!(!s.loaded_from_disk());
    assert_eq!(AppSettings::default().to_json(), s.to_json());
}

#[test]
fn an_unparseable_file_yields_defaults_not_loaded_from_disk() {
    // A settings.json too damaged to parse counts as new: every other preference has just been
    // lost too, so following the Windows theme is the right answer for it as well.
    let path = temp_settings_file();
    std::fs::write(&path, "{ this is not json").unwrap();

    let s = AppSettings::load(&path);

    assert!(!s.loaded_from_disk());
    assert_eq!(AppSettings::default().to_json(), s.to_json());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_json_null_is_not_a_settings_file() {
    let path = temp_settings_file();
    std::fs::write(&path, "null").unwrap();

    assert!(!AppSettings::load(&path).loaded_from_disk());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_that_parses_is_loaded_from_disk() {
    let path = temp_settings_file();
    std::fs::write(&path, "{\"showHiddenItems\": true}").unwrap();

    let s = AppSettings::load(&path);

    assert!(s.loaded_from_disk());
    assert!(s.show_hidden_items);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_empty_object_is_still_a_file_from_disk() {
    let s = AppSettings::from_json("{}").unwrap();
    assert!(
        !s.loaded_from_disk(),
        "from_json alone says nothing about a disk"
    );

    let path = temp_settings_file();
    std::fs::write(&path, "{}").unwrap();
    assert!(AppSettings::load(&path).loaded_from_disk());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_unknown_field_is_ignored() {
    let s = AppSettings::from_json("{\"someFutureSetting\": 42, \"scrollSpeedMultiplier\": 3.5}")
        .unwrap();

    assert_eq!(3.5, s.scroll_speed_multiplier);
}

#[test]
fn a_missing_field_keeps_its_default() {
    let s = AppSettings::from_json("{\"showHiddenItems\": true}").unwrap();

    assert_eq!(2.0, s.scroll_speed_multiplier);
    assert_eq!(360.0, s.preview_pane_width);
    assert!(s.enter_archives_on_double_click);
    assert_eq!("Sha256", s.checksum_algorithms);
}

#[test]
fn the_json_is_camel_cased_with_named_enums() {
    let json = AppSettings::default().to_json();

    assert!(json.contains("\"showHiddenItems\": false"), "{json}");
    assert!(!json.contains("ShowHiddenItems"), "{json}");
    assert!(json.contains("\"drivesOpenTarget\": \"NewTab\""), "{json}");
    assert!(
        json.contains("\"workspacesPlacement\": \"Sidebar\""),
        "{json}"
    );
}

#[test]
fn loaded_from_disk_is_not_persisted() {
    let path = temp_settings_file();
    std::fs::write(&path, "{}").unwrap();
    let s = AppSettings::load(&path);
    assert!(s.loaded_from_disk());

    let json = s.to_json();

    assert!(!json.to_lowercase().contains("loadedfromdisk"), "{json}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_round_trip_is_lossless() {
    let mut s = AppSettings::default();
    s.window_left = Some(10.5);
    s.window_top = Some(20.0);
    s.window_width = Some(1280.0);
    s.window_height = Some(720.0);
    s.window_maximized = true;
    s.last_path = Some(r"C:\Users\Rob".into());
    s.session = Some(SessionLayout::pane(&[r"C:\A", r"C:\B"]));
    s.restore_last_session = false;
    s.startup_default_path = Some(r"D:\Start".into());
    s.show_hidden_items = true;
    s.directory_thumbnail_scales
        .insert(r"C:\PHOTOS".into(), 0.75);
    s.scroll_speed_multiplier = 1.0;
    s.show_preview_pane = true;
    s.preview_pane_width = 400.0;
    s.file_list_columns = Some(vec![
        ColumnSetting::new("name", 300.0),
        ColumnSetting::new("System.Photo.DateTaken", 120.0),
    ]);
    s.checksum_algorithms = "Md5,Sha256".into();
    s.record_file_changes = true;
    s.file_change_retention_hours = 168;
    s.change_timeline_mark_utc = Some(FileTime::from_ymd_hms(2026, 10, 23, 9, 30, 0));
    s.undo_max_steps = Some(100);
    s.undo_max_held_gigabytes = Some(25);
    s.tile_aspect_ratio = "16:9".into();
    s.custom_commands.push(CustomCommandDefinition {
        id: "abc123".into(),
        show_in_menu: false,
        name: "Open in Notepad".into(),
        command: "notepad.exe".into(),
        arguments: "\"{path}\"".into(),
        applies_to_files: true,
        applies_to_directories: false,
        run_elevated: true,
    });
    let mut bindings = BTreeMap::new();
    bindings.insert("tab.new".to_string(), vec!["Ctrl+N".to_string()]);
    bindings.insert("tab.close".to_string(), Vec::new());
    s.key_bindings = Some(bindings);
    s.palette_pins = vec!["workspace:Work".into(), "tab.new".into()];
    s.show_shell_extensions = false;
    s.hidden_shell_extensions = vec!["clsid:{123}".into()];
    s.hidden_built_in_menu_items = vec!["copy-path".into()];
    s.context_menu_layout = Some(vec!["app:cut".into(), "-".into(), "@more".into()]);
    s.theme_id = Some("dark-plus".into());
    s.follow_system_theme = Some(false);
    s.light_theme_id = Some("light-plus".into());
    s.dark_theme_id = Some("my-dark".into());
    let mut tweaks = BTreeMap::new();
    tweaks.insert("Theme.Accent".to_string(), "#FF0000".to_string());
    s.theme_overrides.insert("dark-plus".into(), tweaks);
    s.advanced_rename_expanded = true;
    s.advanced_rename = Some(RenameRule {
        template: "{name}".into(),
        find: String::new(),
        replace: String::new(),
        use_regex: true,
        match_case: false,
        scope: RenameScope::WholeName,
        case: RenameCase::Title,
        counter_start: 5,
        counter_step: 2,
        is_literal: false,
    });
    s.enter_archives_on_double_click = false;
    s.drives_open_target = DrivesOpenTarget::NewPanel;
    s.workspaces_placement = SectionPlacement::TitleBar;
    s.saved_searches_placement = SectionPlacement::Hidden;

    let path = temp_settings_file();
    s.save(&path).unwrap();
    let loaded = AppSettings::load(&path);

    assert!(loaded.loaded_from_disk());
    assert_eq!(s.to_json(), loaded.to_json());
    assert_eq!(Some(1.0), loaded.session.as_ref().map(|l| l.weight));
    assert_eq!(
        Some(RenameScope::WholeName),
        loaded.advanced_rename.as_ref().map(|r| r.scope)
    );
    let json = std::fs::read_to_string(&path).unwrap();
    assert!(json.contains("\"scope\": \"WholeName\""), "{json}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_creates_the_directory() {
    let dir = unique_temp_path("bertbrowser-settings-dir", "");
    let path = dir.join("nested").join("settings.json");

    AppSettings::default().save(&path).unwrap();

    assert!(path.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- the derived answers ---

#[test]
fn following_the_windows_theme_is_the_default_only_because_there_is_no_file() {
    assert!(AppSettings::default().follows_system_theme());

    let path = temp_settings_file();
    std::fs::write(&path, "{}").unwrap();
    assert!(
        !AppSettings::load(&path).follows_system_theme(),
        "an existing user who never opened Settings is not flipped on upgrade"
    );

    std::fs::write(&path, "{\"followSystemTheme\": true}").unwrap();
    assert!(AppSettings::load(&path).follows_system_theme());

    let mut pinned = AppSettings::default();
    pinned.follow_system_theme = Some(false);
    assert!(!pinned.follows_system_theme());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_change_log_policy_is_off_until_recording_is_on() {
    let mut s = AppSettings::default();
    assert_eq!(ChangeLogPolicy::OFF, s.effective_change_log_policy());

    s.record_file_changes = true;
    assert_eq!(
        ChangeLogPolicy::from_hours(24),
        Some(s.effective_change_log_policy())
    );

    s.file_change_retention_hours = 168;
    assert_eq!(
        ChangeLogPolicy::from_hours(168),
        Some(s.effective_change_log_policy())
    );
}

#[test]
fn a_retention_no_longer_on_the_menu_falls_back_to_the_default() {
    let mut s = AppSettings::default();
    s.record_file_changes = true;

    for hours in [5, 0, -1, 1000] {
        s.file_change_retention_hours = hours;
        assert_eq!(
            ChangeLogPolicy::from_hours(ChangeLogPolicy::DEFAULT_RETENTION_HOURS),
            Some(s.effective_change_log_policy()),
            "{hours}"
        );
    }
}

#[test]
fn the_undo_budget_takes_the_defaults_for_null_and_lifts_nonsense_to_one() {
    let mut s = AppSettings::default();
    let budget = s.effective_undo_budget();
    assert_eq!(50, budget.max_entries);
    assert_eq!(10i64 << 30, budget.max_held_bytes);

    s.undo_max_steps = Some(0);
    s.undo_max_held_gigabytes = Some(-3);
    let budget = s.effective_undo_budget();
    assert_eq!(1, budget.max_entries);
    assert_eq!(1i64 << 30, budget.max_held_bytes);
}

#[test]
fn the_checksum_algorithms_are_parsed_normalised_and_never_empty() {
    let mut s = AppSettings::default();
    assert_eq!(
        vec![ChecksumAlgorithm::Sha256],
        s.resolved_checksum_algorithms()
    );

    s.checksum_algorithms = " crc32 , md5, bogus,, MD5".into();
    assert_eq!(
        vec![ChecksumAlgorithm::Md5, ChecksumAlgorithm::Crc32],
        s.resolved_checksum_algorithms(),
        "in the offered order, once each, unknown names dropped"
    );

    for unusable in ["", "   ", "bogus", ","] {
        s.checksum_algorithms = unusable.into();
        assert_eq!(
            vec![ChecksumAlgorithm::Sha256],
            s.resolved_checksum_algorithms(),
            "{unusable:?}"
        );
    }
}

#[test]
fn the_new_file_types_ship_defaults_until_configured() {
    let mut s = AppSettings::default();
    let shipped: Vec<String> = s
        .resolved_new_file_types()
        .iter()
        .map(|t| t.extension.clone())
        .collect();
    assert_eq!(vec![".txt", ".md", ".json"], shipped);

    // An empty list means they removed them all on purpose.
    s.new_file_types = Some(Vec::new());
    assert!(s.resolved_new_file_types().is_empty());

    let mut disabled = bb_core::settings::NewFileTemplate::new("Old", ".old");
    disabled.enabled = false;
    s.new_file_types = Some(vec![
        bb_core::settings::NewFileTemplate::new("Blank", ""),
        disabled,
        bb_core::settings::NewFileTemplate::new("Script", ".ps1"),
    ]);
    let resolved = s.resolved_new_file_types();
    assert_eq!(1, resolved.len());
    assert_eq!("New Script", resolved[0].default_base_name());
}

#[test]
fn the_resolved_file_list_columns_go_through_the_layout_rules() {
    use bb_core::columns::{ColumnLayoutRules, ColumnSetting};

    // Nothing customised: the shipped defaults, normalised.
    let settings = AppSettings::default();
    assert_eq!(
        ColumnLayoutRules::normalize(None),
        settings.resolved_file_list_columns()
    );
    assert!(!settings.resolved_file_list_columns().is_empty());

    // A hand-edited file with a nonsense width is repaired rather than shown as written.
    let mut customised = AppSettings::default();
    let mut odd = ColumnLayoutRules::normalize(None);
    odd[0].width = 0.0;
    customised.file_list_columns = Some(odd.clone());
    let resolved = customised.resolved_file_list_columns();
    assert_eq!(ColumnLayoutRules::normalize(Some(&odd)), resolved);
    assert!(
        resolved
            .iter()
            .all(|c: &ColumnSetting| c.width >= ColumnLayoutRules::MIN_WIDTH)
    );
}
