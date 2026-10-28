// Port of tests/BertBrowser.Core.Tests/ShellNewImportTests.cs. The half of the ShellNew read that
// has judgement in it — which is why it lives in Core, in a crate that cannot open a registry key.
// The rules worth guarding are what gets dropped (a registry-supplied command line, above all) and
// what a label falls back to when Windows will not tell us one.
use std::collections::HashSet;

use bb_core::newitem::{NewFileTemplate, ShellNewEntry, ShellNewImport, ShellNewKind};

struct Setup {
    files: HashSet<String>,
    roots: Vec<String>,
}

impl Setup {
    fn new() -> Setup {
        Setup {
            files: HashSet::new(),
            roots: vec![r"C:\Templates".to_string()],
        }
    }

    fn add_file(&mut self, path: &str) {
        self.files.insert(path.to_uppercase());
    }

    /// With a resolver that can never load anything — the interesting case.
    fn import(&self, entries: &[ShellNewEntry]) -> Vec<NewFileTemplate> {
        self.import_with(entries, |_| None)
    }

    fn import_with(
        &self,
        entries: &[ShellNewEntry],
        resolver: impl Fn(&str) -> Option<String>,
    ) -> Vec<NewFileTemplate> {
        ShellNewImport::to_templates(
            entries,
            resolver,
            |path| self.files.contains(&path.to_uppercase()),
            &self.roots,
            |extension, _| Some(format!(r"C:\saved\{}", extension.trim_start_matches('.'))),
        )
    }
}

fn entry(extension: &str, label: Option<&str>, kind: ShellNewKind) -> ShellNewEntry {
    ShellNewEntry::new(extension, label, kind)
}

fn single(templates: Vec<NewFileTemplate>) -> NewFileTemplate {
    assert_eq!(1, templates.len(), "{templates:?}");
    templates.into_iter().next().unwrap()
}

// --- what is dropped ---

#[test]
fn a_command_entry_is_dropped_because_it_would_run_a_program() {
    let entries = [
        entry(".lnk", Some("Shortcut"), ShellNewKind::Command),
        entry(".txt", Some("Text Document"), ShellNewKind::NullFile),
    ];

    let templates = Setup::new().import(&entries);

    assert_eq!(
        vec![".txt"],
        templates
            .iter()
            .map(|t| t.extension.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_template_in_none_of_the_roots_is_dropped_rather_than_offering_a_type_that_would_always_refuse()
{
    let entry = ShellNewEntry {
        file_name: Some("winword.rtf".to_string()),
        ..entry(".rtf", Some("Rich Text"), ShellNewKind::FileName)
    };
    assert!(Setup::new().import(&[entry]).is_empty());
}

#[test]
fn an_extension_that_could_not_end_a_legal_file_name_is_dropped() {
    for extension in ["", "txt", ".", ".tar.gz", ".a?b"] {
        assert!(
            Setup::new()
                .import(&[entry(extension, Some("Something"), ShellNewKind::NullFile)])
                .is_empty(),
            "{extension:?}"
        );
    }
}

#[test]
fn two_entries_for_one_extension_keep_only_the_first() {
    let entries = [
        entry(".txt", Some("Text Document"), ShellNewKind::NullFile),
        entry(".TXT", Some("Something Else"), ShellNewKind::NullFile),
    ];

    let templates = Setup::new().import(&entries);

    assert_eq!("Text Document", single(templates).label);
}

// --- labels ---

#[test]
fn an_indirect_string_is_resolved_through_the_resolver() {
    let entry = entry(
        ".txt",
        Some(r"@%SystemRoot%\system32\notepad.exe,-469"),
        ShellNewKind::NullFile,
    );

    let templates = Setup::new().import_with(&[entry], |_| Some("Text Document".to_string()));

    assert_eq!("Text Document", single(templates).label);
}

#[test]
fn an_indirect_string_the_resolver_cannot_load_falls_back_to_the_bare_extension() {
    // Never the raw "@%SystemRoot%\..." text: that is the failure a user would actually see.
    let entry = entry(
        ".txt",
        Some(r"@%SystemRoot%\system32\notepad.exe,-469"),
        ShellNewKind::NullFile,
    );

    assert_eq!("TXT File", single(Setup::new().import(&[entry])).label);
}

#[test]
fn an_entry_with_no_label_at_all_falls_back_to_the_bare_extension() {
    assert_eq!(
        "MD File",
        single(Setup::new().import(&[entry(".md", None, ShellNewKind::NullFile)])).label
    );
}

// --- templates ---

#[test]
fn a_bare_template_name_is_resolved_against_the_template_folders() {
    let mut setup = Setup::new();
    setup.add_file(r"C:\Templates\winword.rtf");
    let entry = ShellNewEntry {
        file_name: Some("winword.rtf".to_string()),
        ..entry(".rtf", Some("Rich Text"), ShellNewKind::FileName)
    };

    assert_eq!(
        Some(r"C:\Templates\winword.rtf".to_string()),
        single(setup.import(&[entry])).template_path
    );
}

#[test]
fn a_rooted_template_name_is_taken_as_given() {
    let mut setup = Setup::new();
    setup.add_file(r"D:\Other\sheet.xlsx");
    let entry = ShellNewEntry {
        file_name: Some(r"D:\Other\sheet.xlsx".to_string()),
        ..entry(".xlsx", Some("Worksheet"), ShellNewKind::FileName)
    };

    assert_eq!(
        Some(r"D:\Other\sheet.xlsx".to_string()),
        single(setup.import(&[entry])).template_path
    );
}

#[test]
fn a_null_file_entry_carries_no_template_because_it_is_meant_to_be_empty() {
    assert_eq!(
        None,
        single(Setup::new().import(&[entry(".txt", Some("Text"), ShellNewKind::NullFile)]))
            .template_path
    );
}

#[test]
fn a_data_entry_is_written_out_once_and_carried_as_a_path() {
    let entry = ShellNewEntry {
        data: Some(vec![1, 2, 3]),
        ..entry(".xml", Some("XML"), ShellNewKind::Data)
    };
    assert_eq!(
        Some(r"C:\saved\xml".to_string()),
        single(Setup::new().import(&[entry])).template_path
    );
}

// --- merging into what the user already has ---

#[test]
fn importing_keeps_the_types_the_user_already_has_and_their_order() {
    let existing = vec![
        NewFileTemplate::new("Mine", ".md"),
        NewFileTemplate::new("Also mine", ".txt"),
    ];
    let imported = vec![NewFileTemplate::new("Theirs", ".json")];

    let merged = ShellNewImport::merge(&existing, &imported);

    assert_eq!(
        vec![".md", ".txt", ".json"],
        merged
            .iter()
            .map(|t| t.extension.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!("Mine", merged[0].label);
}

#[test]
fn importing_adds_nothing_for_an_extension_already_listed() {
    let existing = vec![NewFileTemplate::new("Mine", ".txt")];
    let imported = vec![NewFileTemplate::new("Text Document", ".TXT")];

    let merged = ShellNewImport::merge(&existing, &imported);

    assert_eq!("Mine", single(merged).label);
}
