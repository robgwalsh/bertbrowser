// Port of tests/BertBrowser.Core.Tests/RenameTagTests.cs. The rename tokens that draw on a file's
// own metadata: `{artist}`, `{track:00}`, `{taken:yyyy-MM-dd}`.
use std::collections::HashMap;

use bb_core::rename::{
    MetadataField, RenamePattern, RenamePlanner, RenameProbe, RenameRule, RenameSource,
    RenameTemplate, RenamedName,
};

fn song(name: &str, tags: &[(MetadataField, &str)]) -> RenameSource {
    let mut source = RenameSource::file(&format!(r"C:\music\{name}"));
    source.tags = Some(
        tags.iter()
            .map(|(field, value)| (*field, value.to_string()))
            .collect::<HashMap<_, _>>(),
    );
    source
}

fn one(source: &RenameSource, template: &str) -> RenamedName {
    RenamePattern::apply(std::slice::from_ref(source), &RenameRule::new(template)).remove(0)
}

#[test]
fn a_name_is_built_from_what_the_file_says_about_itself() {
    let song = song(
        "track03.mp3",
        &[
            (MetadataField::Artist, "Björk"),
            (MetadataField::Album, "Homogenic"),
            (MetadataField::Title, "Jóga"),
            (MetadataField::TrackNumber, "2"),
            (MetadataField::Year, "1997"),
        ],
    );

    let named = one(&song, "{track:00} {artist} - {title} ({year}){ext}");

    assert_eq!("02 Björk - Jóga (1997).mp3", named.name);
    assert_eq!(None, named.problem);
    assert_eq!("2 Homogenic.mp3", one(&song, "{track} {album}{ext}").name);
}

#[test]
fn a_date_taken_takes_a_format_and_has_a_sortable_default() {
    let mut photo = RenameSource::file(r"C:\p\IMG_0042.jpg");
    photo.tags = Some(HashMap::from([(
        MetadataField::DateTaken,
        "2021-06-05 04:03:02".to_string(),
    )]));

    assert_eq!(
        "2021-06-05 IMG_0042.jpg",
        one(&photo, "{taken} {name}").name
    );
    assert_eq!(
        "2021-06 0403.jpg",
        one(&photo, "{taken:yyyy-MM HHmm}{ext}").name
    );
}

#[test]
fn a_missing_tag_refuses_that_item_rather_than_leaving_a_hole_in_its_name() {
    let untitled = song("a.mp3", &[(MetadataField::Artist, "Somebody")]);
    let named = one(&untitled, "{artist} - {title}{ext}");

    assert!(
        named
            .problem
            .as_deref()
            .unwrap_or("")
            .contains("has no title")
    );

    // Tags nobody read are the same refusal, never a crash and never an empty name.
    let unread = RenameSource::file(r"C:\music\b.mp3");
    assert!(
        one(&unread, "{artist}{ext}")
            .problem
            .as_deref()
            .unwrap_or("")
            .contains("has no artist")
    );

    // And the planner turns it into a rejection, which blocks the whole batch.
    let probe = NothingExists;
    let plan = RenamePlanner::new(&probe).plan(
        std::slice::from_ref(&untitled),
        &RenameRule::new("{artist} - {title}{ext}"),
    );
    assert!(!plan.has_work());
    assert_eq!(1, plan.rejected.len());
    assert!(plan.rejected[0].message.contains("has no title"));
}

#[test]
fn what_a_tag_holds_is_made_safe_for_a_name() {
    let song = song(
        "a.mp3",
        &[
            (MetadataField::Artist, "AC/DC"),
            (MetadataField::Title, "Who Made Who?: Live"),
        ],
    );

    let named = one(&song, "{artist} - {title}{ext}");

    assert_eq!("AC_DC - Who Made Who__ Live.mp3", named.name);
    assert_eq!(None, RenamePattern::validate(&named.name));
}

#[test]
fn a_tag_token_that_cannot_be_used_says_why_while_it_is_being_typed() {
    for (template, expected) in [
        ("{track:abc}", "not a width"),
        ("{artist:x}", "does not take"),
        ("{taken:d}", "can't hold"),
        ("{nosuchtag}", "not a name template token"),
    ] {
        let problem = RenamePattern::validate_rule(&RenameRule::new(&format!("{template}{{ext}}")))
            .unwrap_or_default();
        assert!(problem.contains(expected), "{template}: {problem}");
    }
}

#[test]
fn only_a_template_that_names_a_tag_asks_for_the_tags_to_be_read() {
    assert!(RenameTemplate::uses_tags("{artist} - {title}{ext}"));
    assert!(RenameTemplate::uses_tags("{TAKEN:yyyy}{ext}"));
    assert!(!RenameTemplate::uses_tags("{name}"));
    assert!(!RenameTemplate::uses_tags("{n:000} {modified}{ext}"));
    assert!(!RenameTemplate::uses_tags("{artist"));
}

struct NothingExists;

impl RenameProbe for NothingExists {
    fn directory_exists(&self, _path: &str) -> bool {
        false
    }

    fn file_exists(&self, path: &str) -> bool {
        path.to_ascii_uppercase().starts_with(r"C:\MUSIC\") && !path.contains(" - ")
    }
}
