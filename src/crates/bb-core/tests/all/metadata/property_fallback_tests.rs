// Port of tests/BertBrowser.Core.Tests/PropertyFallbackTests.cs. The Windows-property fallback as
// far as Core goes: which files it is offered for, the copy it is pointed at, and how values cross.
// The handler itself is Windows' and is exercised by tools/ui/metadata.bbs; here it is a fake that
// keeps a title on a file's first line.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use bb_core::FileTime;
use bb_core::fs::DirectoryRemoval;
use bb_core::metadata::{
    FileSystemMetadataProbe, MetadataDocument, MetadataEdit, MetadataEditExecutor,
    MetadataEditPlan, MetadataEditPlanner, MetadataError, MetadataFamily, MetadataField,
    MetadataFieldKind, MetadataFields, MetadataRejection, MetadataResult, OfferedByWindows,
    PropertyFallback, PropertyFallbackReading, REWRITE_MARKER, WindowsPropertyFields,
};
use bb_test_support::metadata_fixtures as fx;
use bb_test_support::unique_temp_path;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let root = unique_temp_path("bertbrowser-fallback", "");
        std::fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    fn film(&self) -> String {
        self.film_named("film.mov", "Old")
    }

    fn film_named(&self, name: &str, title: &str) -> String {
        let path = self.root.join(name).to_string_lossy().into_owned();
        std::fs::write(&path, format!("{title}\nthe frames")).unwrap();
        path
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(&self.root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".bertbrowser-"))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    let mut permissions = meta.permissions();
                    if permissions.readonly() {
                        #[allow(clippy::permissions_set_readonly_false)]
                        permissions.set_readonly(false);
                        let _ = std::fs::set_permissions(entry.path(), permissions);
                    }
                }
            }
        }
        let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
    }
}

/// A "format" of one title line and then a body nothing should touch.
#[derive(Default)]
struct FakeHandler {
    written: RefCell<Vec<String>>,
    corrupt: Option<fn(&str) -> String>,
    refuses: bool,
}

impl PropertyFallback for FakeHandler {
    fn read(&self, path: &str) -> Option<PropertyFallbackReading> {
        let text = std::fs::read_to_string(path).ok()?;
        let (first, _) = text.split_once('\n')?;
        let mut values = BTreeMap::new();
        if !first.is_empty() {
            values.insert(MetadataField::Title, first.to_string());
        }
        Some(PropertyFallbackReading {
            document: MetadataDocument::new(MetadataFamily::Document, values),
            fields: vec![MetadataField::Title],
        })
    }

    fn write(&self, path: &str, changes: &BTreeMap<MetadataField, String>) -> MetadataResult<()> {
        self.written.borrow_mut().push(path.to_string());
        if self.refuses {
            return Err(MetadataError::format("Windows could not write to it."));
        }

        let text = std::fs::read_to_string(path)?;
        let body = text.split_once('\n').map(|(_, b)| b).unwrap_or("");
        let title = &changes[&MetadataField::Title];
        let written = match self.corrupt {
            Some(corrupt) => corrupt(title),
            None => title.clone(),
        };
        std::fs::write(path, format!("{written}\n{body}"))?;
        Ok(())
    }
}

fn title_edit(title: &str) -> MetadataEdit {
    MetadataEdit::of(&[
        (MetadataField::Title, title),
        (MetadataField::Album, "Not offered"),
    ])
}

fn plan(path: &str, edit: MetadataEdit) -> MetadataEditPlan {
    let offered: OfferedByWindows =
        BTreeMap::from([(path.to_string(), vec![MetadataField::Title])]);
    MetadataEditPlanner::local().plan(&[path.to_string()], &edit, Some(&offered))
}

fn executor(handler: &FakeHandler) -> MetadataEditExecutor<'_> {
    static PROBE: FileSystemMetadataProbe = FileSystemMetadataProbe;
    MetadataEditExecutor::new(&PROBE, Some(handler))
}

fn run(
    executor: &MetadataEditExecutor<'_>,
    plan: &MetadataEditPlan,
) -> bb_core::metadata::MetadataEditOutcome {
    executor.execute(plan, &bb_core::cancel::CancelToken::none(), None, None)
}

#[test]
fn a_file_with_no_codec_is_planned_only_when_windows_offered_fields_for_it() {
    let x = Fixture::new();
    let film = x.film();

    let without = MetadataEditPlanner::local().plan(
        std::slice::from_ref(&film),
        &MetadataEdit::of(&[(MetadataField::Title, "New")]),
        None,
    );
    assert_eq!(1, without.rejected.len());
    assert_eq!(MetadataRejection::Unsupported, without.rejected[0].reason);

    let planned = plan(&film, title_edit("New"));
    assert_eq!(1, planned.files.len());
    assert!(planned.files[0].via_windows);
    let fields: Vec<MetadataField> = planned.files[0].edit.changes.keys().copied().collect();
    assert_eq!(vec![MetadataField::Title], fields);
}

#[test]
fn a_codec_always_wins_over_the_fallback() {
    let x = Fixture::new();
    let photo = x.root.join("photo.jpg").to_string_lossy().into_owned();
    std::fs::write(&photo, fx::jpeg(fx::PLAIN_JPEG)).unwrap();

    let planned = plan(&photo, title_edit("New"));
    assert_eq!(1, planned.files.len());
    assert!(!planned.files[0].via_windows);
}

#[test]
fn the_fallback_takes_fields_and_never_a_removal() {
    let x = Fixture::new();
    let film = x.film();

    let planned = plan(
        &film,
        MetadataEdit {
            remove_all: true,
            ..MetadataEdit::none()
        },
    );

    assert!(!planned.has_work());
    assert_eq!(1, planned.rejected.len());
    assert_eq!(
        MetadataRejection::NothingApplies,
        planned.rejected[0].reason
    );
}

#[test]
fn every_other_refusal_still_stands() {
    let x = Fixture::new();
    let film = x.film();
    let mut permissions = std::fs::metadata(&film).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&film, permissions).unwrap();

    let planned = plan(&film, title_edit("New"));
    assert_eq!(1, planned.rejected.len());
    assert_eq!(MetadataRejection::ReadOnly, planned.rejected[0].reason);
    assert_eq!(
        MetadataRejection::InsideArchive,
        MetadataEditPlanner::refusal(r"C:\p\pack.zip\film.mov", None, true)
            .unwrap()
            .reason
    );
}

#[test]
fn the_handler_writes_a_copy_and_the_original_is_held_for_undo() {
    let x = Fixture::new();
    let film = x.film();
    let before = std::fs::read(&film).unwrap();
    let handler = FakeHandler::default();
    let executor = executor(&handler);

    let outcome = run(&executor, &plan(&film, title_edit("New")));

    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(1, outcome.edited.len());
    let held = &outcome.edited[0];
    assert_eq!("New\nthe frames", std::fs::read_to_string(&film).unwrap());
    assert_eq!(before, std::fs::read(&held.held_path).unwrap());

    // Never the user's own file: the handler was pointed at the sibling, with its extension.
    let written = handler.written.borrow();
    assert_eq!(1, written.len());
    assert_ne!(film, written[0]);
    assert!(written[0].contains(REWRITE_MARKER), "{}", written[0]);
    assert!(written[0].ends_with(".mov"), "{}", written[0]);

    let undone = executor.undo(&outcome.edited);
    assert!(undone.failed.is_empty(), "{:?}", undone.failed);
    assert_eq!(before, std::fs::read(&film).unwrap());

    MetadataEditExecutor::commit_staging(&undone.swapped);
    assert!(x.leftovers().is_empty());
}

#[test]
fn a_handler_that_refuses_costs_a_temporary_file_and_nothing_else() {
    let x = Fixture::new();
    let film = x.film();
    let before = std::fs::read(&film).unwrap();
    let handler = FakeHandler {
        refuses: true,
        ..FakeHandler::default()
    };

    let outcome = run(&executor(&handler), &plan(&film, title_edit("New")));

    assert!(outcome.edited.is_empty());
    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0].message.contains("Windows"),
        "{}",
        outcome.failed[0].message
    );
    assert_eq!(before, std::fs::read(&film).unwrap());
    assert!(x.leftovers().is_empty());
}

#[test]
fn a_value_that_does_not_read_back_is_not_swapped_in() {
    let x = Fixture::new();
    let film = x.film();
    let before = std::fs::read(&film).unwrap();
    let handler = FakeHandler {
        corrupt: Some(|title| title[..1].to_string()),
        ..FakeHandler::default()
    };

    let outcome = run(&executor(&handler), &plan(&film, title_edit("New")));

    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0].message.contains("did not read back"),
        "{}",
        outcome.failed[0].message
    );
    assert_eq!(before, std::fs::read(&film).unwrap());
    assert!(x.leftovers().is_empty());
}

#[test]
fn writing_what_is_already_there_is_unchanged_and_not_a_history_entry() {
    let x = Fixture::new();
    let film = x.film_named("film.mov", "Same");
    let handler = FakeHandler::default();

    let outcome = run(&executor(&handler), &plan(&film, title_edit("Same")));

    assert!(!outcome.can_undo());
    assert_eq!(vec![film], outcome.unchanged);
    assert!(x.leftovers().is_empty());
}

#[test]
fn without_a_fallback_the_file_fails_by_name_rather_than_throwing() {
    let x = Fixture::new();
    let film = x.film();

    let outcome = run(
        &MetadataEditExecutor::local(),
        &plan(&film, title_edit("New")),
    );

    assert_eq!(1, outcome.failed.len());
    assert!(
        outcome.failed[0].message.contains("film.mov"),
        "{}",
        outcome.failed[0].message
    );
    assert!(x.leftovers().is_empty());
}

// ---- values -----------------------------------------------------------------------------------

#[test]
fn stars_cross_as_the_numbers_explorer_writes_and_come_back_the_same() {
    for (stars, rating) in [("1", 1u32), ("2", 25), ("3", 50), ("4", 75), ("5", 99)] {
        assert_eq!(
            Some(rating),
            WindowsPropertyFields::to_windows(MetadataField::Rating, stars)
                .unwrap()
                .number,
            "{stars}"
        );
        assert_eq!(
            stars,
            WindowsPropertyFields::from_windows(
                MetadataField::Rating,
                None,
                Some(rating as f64),
                None
            ),
            "{stars}"
        );
    }
}

#[test]
fn a_rating_another_program_wrote_lands_in_the_band_explorer_shows_it_in() {
    let band = |rating: f64| {
        WindowsPropertyFields::from_windows(MetadataField::Rating, None, Some(rating), None)
    };
    assert_eq!("", band(0.0));
    assert_eq!("1", band(12.0));
    assert_eq!("2", band(13.0));
    assert_eq!("4", band(87.0));
    assert_eq!("5", band(88.0));
}

#[test]
fn a_date_crosses_as_universal_time_and_comes_back_as_it_was_typed() {
    let typed = "2024-03-14 09:26:53";
    let local = MetadataFields::try_parse_date(typed).unwrap();

    let sent = WindowsPropertyFields::to_windows(MetadataField::DateTaken, typed).unwrap();

    let utc: FileTime = sent.utc.expect("a date crosses as a time");
    assert_eq!(local.local_to_utc(), utc);
    assert_eq!(
        typed,
        WindowsPropertyFields::from_windows(
            MetadataField::DateTaken,
            Some("whatever Windows renders"),
            None,
            Some(utc)
        )
    );
}

#[test]
fn text_lists_and_numbers_cross_as_themselves_and_an_empty_value_clears() {
    let to = |field, value| WindowsPropertyFields::to_windows(field, value).unwrap();
    assert_eq!(
        Some("A title".to_string()),
        to(MetadataField::Title, "A title").text
    );
    assert_eq!(
        Some("sea; boats".to_string()),
        to(MetadataField::Keywords, "sea; boats").text
    );
    assert_eq!(Some(1999), to(MetadataField::Year, "1999").number);
    assert!(to(MetadataField::Title, "").is_clear());
    assert!(to(MetadataField::Rating, "0").is_clear());

    let from = |field, text, number| WindowsPropertyFields::from_windows(field, text, number, None);
    assert_eq!(
        "sea; boats",
        from(MetadataField::Keywords, Some("sea;boats"), None)
    );
    assert_eq!(
        "1999",
        from(MetadataField::Year, Some("1,999"), Some(1999.0))
    );
    assert_eq!("", from(MetadataField::Year, Some("not a year"), None));
    assert!(matches!(
        WindowsPropertyFields::to_windows(MetadataField::Year, "soon"),
        Err(e) if e.is_format()
    ));
}

#[test]
fn every_mapped_field_is_one_whose_value_can_cross() {
    let canonical = WindowsPropertyFields::canonical();
    for (field, name) in canonical {
        let kind = MetadataFields::get(*field).kind;
        assert!(
            [
                MetadataFieldKind::Text,
                MetadataFieldKind::List,
                MetadataFieldKind::Number,
                MetadataFieldKind::Date,
                MetadataFieldKind::Rating,
            ]
            .contains(&kind),
            "{field:?}"
        );
        assert!(name.starts_with("System."), "{name}");
    }

    let distinct: HashSet<&str> = canonical.iter().map(|(_, n)| *n).collect();
    assert_eq!(canonical.len(), distinct.len());

    assert_eq!(
        Some(MetadataField::Title),
        WindowsPropertyFields::field_of("system.title")
    );
    assert_eq!(
        Some(MetadataField::Iso),
        WindowsPropertyFields::field_of("System.Photo.ISOSpeed")
    );
    assert_eq!(
        None,
        WindowsPropertyFields::field_of("System.Image.Dimensions")
    );
}
