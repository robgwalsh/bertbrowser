// Port of tests/BertBrowser.Core.Tests/ColumnCatalogTests.cs.
use std::collections::HashSet;

use bb_core::columns::{ColumnCatalog, ColumnId, ColumnKind, preview_metadata};
use bb_paths::fold_upper;

// --- The two id spaces ---
//
// Everything about how an unknown id is treated rests on being able to tell a built-in id from a
// canonical property name without a lookup. If these two ever overlap, a column from a newer
// build stops being droppable and a property this machine lacks stops being keepable.

#[test]
fn no_built_in_id_looks_like_a_canonical_name() {
    for spec in ColumnCatalog::built_ins() {
        assert!(!ColumnId::looks_canonical(&spec.id), "{}", spec.id);
    }
}

#[test]
fn every_curated_id_looks_like_a_canonical_name() {
    for spec in ColumnCatalog::curated() {
        assert!(ColumnId::looks_canonical(&spec.id), "{}", spec.id);
    }
}

#[test]
fn a_canonical_name_is_recognised() {
    for id in [
        "System.Photo.DateTaken",
        "System.Size",
        "System.Image.Dimensions",
    ] {
        assert!(ColumnId::looks_canonical(id), "{id}");
    }
}

#[test]
fn anything_else_is_not() {
    let rows: [(Option<&str>, &str); 8] = [
        (None, "null"),
        (Some(""), "empty"),
        (Some("Name"), "a built-in, no dot"),
        (Some(".System"), "leading dot"),
        (Some("System."), "trailing dot"),
        (Some("System.Photo Date"), "whitespace"),
        (
            Some("System.Photo-Date"),
            "punctuation a canonical name never carries",
        ),
        (
            Some("C:\\Users\\notes.txt"),
            "a path, which is the shape that must never pass",
        ),
    ];
    for (id, why) in rows {
        assert!(
            !ColumnId::looks_canonical(id.unwrap_or("")),
            "{id:?}: {why}"
        );
    }
}

#[test]
fn an_id_too_long_to_be_a_registered_name_is_refused() {
    let id = format!("System.{}", "x".repeat(ColumnId::MAX_LENGTH));
    assert!(!ColumnId::looks_canonical(&id));
}

// --- What is on offer ---

#[test]
fn the_defaults_are_todays_columns_at_todays_widths() {
    let defaults = ColumnCatalog::defaults();

    let ids: Vec<&str> = defaults.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(vec!["Name", "Size", "Type", "Modified"], ids);
    let widths: Vec<f64> = defaults.iter().map(|c| c.width).collect();
    assert_eq!(vec![320.0, 110.0, 120.0, 140.0], widths);
}

#[test]
fn the_curated_set_is_composed_from_the_preview_panes_own_lists() {
    // Not retyped: 38 canonical strings that would otherwise drift with nothing to notice.
    let curated: HashSet<String> = ColumnCatalog::curated()
        .iter()
        .map(|s| fold_upper(&s.id))
        .collect();

    for canonical in preview_metadata::IMAGE_ORDER
        .iter()
        .chain(preview_metadata::MEDIA_ORDER)
        .chain(preview_metadata::DOCUMENT_ORDER)
    {
        if ColumnCatalog::is_shadowed_by_built_in(canonical) {
            continue;
        }
        assert!(curated.contains(&fold_upper(canonical)), "{canonical}");
    }
}

#[test]
fn the_curated_set_offers_nothing_a_built_in_already_says() {
    // Two Type columns — "PNG file" beside "PNG image", one of them blank until it hydrates — is
    // worse than one, and the free one always wins.
    for spec in ColumnCatalog::curated() {
        assert!(
            !ColumnCatalog::is_shadowed_by_built_in(&spec.id),
            "{}",
            spec.id
        );
    }
}

#[test]
fn there_are_no_duplicates_in_the_curated_set() {
    // Media and Document both list System.Title.
    let distinct: HashSet<String> = ColumnCatalog::curated()
        .iter()
        .map(|s| fold_upper(&s.id))
        .collect();
    assert_eq!(ColumnCatalog::curated().len(), distinct.len());
}

#[test]
fn a_curated_column_header_falls_back_to_the_tail_of_its_key() {
    assert_eq!(
        "Dimensions",
        ColumnCatalog::try_get("System.Image.Dimensions")
            .unwrap()
            .header
    );
}

// --- Resolving an id ---

#[test]
fn an_unknown_bare_word_has_no_spec() {
    assert!(ColumnCatalog::try_get("Colour").is_none());
}

#[test]
fn an_uncurated_canonical_name_gets_a_synthesized_spec() {
    let spec = ColumnCatalog::try_get("System.Contact.NickName");

    let spec = spec.expect("a spec");
    assert_eq!(ColumnKind::ShellProperty, spec.kind);
    assert_eq!("NickName", spec.header);
}

#[test]
fn sorting_by_an_unusable_id_degrades_to_name_rather_than_failing() {
    assert_eq!("Name", ColumnCatalog::sort_spec(Some("Colour")).id);
    assert_eq!("Name", ColumnCatalog::sort_spec(None).id);
    assert_eq!("Name", ColumnCatalog::sort_spec(Some("")).id);
}

#[test]
fn sorting_by_match_degrades_to_name_because_match_is_not_sortable() {
    assert_eq!("Name", ColumnCatalog::sort_spec(Some("Match")).id);
}

#[test]
fn the_sort_ids_every_previous_build_wrote_still_resolve() {
    // SessionTab.SortBy has always held these exact strings. Renaming one silently retires a
    // saved sort order, so this is the guard on that.
    for id in ["Name", "Size", "Type", "Modified", "RelativePath"] {
        assert_eq!(id, ColumnCatalog::sort_spec(Some(id)).id);
    }
}
