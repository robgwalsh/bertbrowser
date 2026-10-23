//! Every column this app can show: the built-ins it answers itself, and the curated shell
//! properties it offers on the header menu. One place, so the menu, the picker, the settings page
//! and the harness cannot disagree about what is on offer.
//! Spec: Core/Services/Columns/ColumnCatalog.cs.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use bb_paths::fold_upper;

use super::preview_metadata::{DOCUMENT_ORDER, IMAGE_ORDER, MEDIA_ORDER, shorten_canonical};
use super::{ColumnId, ColumnSetting, ColumnSpec};

pub struct ColumnCatalog;

impl ColumnCatalog {
    // Built-in ids. These strings are load-bearing twice over: they are the header tags the click
    // handler reads, and the values written into a saved tab's sort order. Renaming one silently
    // retires a saved sort order.
    pub const NAME: &'static str = "Name";
    pub const SIZE: &'static str = "Size";
    pub const TYPE: &'static str = "Type";
    pub const MODIFIED: &'static str = "Modified";
    pub const CREATED: &'static str = "Created";
    pub const ACCESSED: &'static str = "Accessed";
    pub const ATTRIBUTES: &'static str = "Attributes";
    pub const EXTENSION: &'static str = "Extension";
    pub const RELATIVE_PATH: &'static str = "RelativePath";
    pub const MATCH: &'static str = "Match";
    /// How this row stands against the other pane, while a comparison is running. Injected
    /// because colour alone is not something every reader can use, and because the tints are
    /// necessarily faint.
    pub const COMPARE_STATUS: &'static str = "CompareStatus";

    /// Shell properties that would sit beside a built-in saying nearly the same thing. Two Type
    /// columns — the built-in's "PNG file" and `System.ItemTypeText`'s "PNG image" — is worse than
    /// one, and the shell one is blank until it hydrates while the free one never is.
    pub const SHADOWED_BY_BUILT_IN: &'static [&'static str] = &[
        "System.ItemNameDisplay",
        "System.ItemTypeText",
        "System.Size",
        "System.DateModified",
        "System.DateCreated",
        "System.DateAccessed",
        "System.FileAttributes",
        "System.FileExtension",
        "System.ItemFolderPathDisplay",
    ];

    /// Columns the app places itself, following the list's mode rather than anyone's choice. They
    /// are never persisted and never appear on the header menu.
    pub fn is_injected(id: &str) -> bool {
        id == Self::RELATIVE_PATH || id == Self::MATCH || id == Self::COMPARE_STATUS
    }

    pub fn built_ins() -> &'static [ColumnSpec] {
        &BUILT_INS
    }

    /// Case-insensitive, as every id comparison in this module is.
    pub fn is_shadowed_by_built_in(canonical: &str) -> bool {
        SHADOWED.contains(&fold_upper(canonical))
    }

    /// The shell properties offered directly on the header menu, grouped. Everything else on the
    /// machine is reachable through "More columns…".
    pub fn curated() -> &'static [ColumnSpec] {
        &CURATED
    }

    /// A spec for a canonical name, curated or not. The header is the tail of the key; the real
    /// localised one replaces it when the column is added.
    pub fn spec_for_property(canonical: &str, group: &str) -> ColumnSpec {
        ColumnSpec::shell_property(canonical, shorten_canonical(canonical), 140.0, group)
    }

    /// Today's columns, at today's widths — what a profile that has never configured them shows.
    pub fn defaults() -> Vec<ColumnSetting> {
        vec![
            ColumnSetting::new(Self::NAME, 320.0),
            ColumnSetting::new(Self::SIZE, 110.0),
            ColumnSetting::new(Self::TYPE, 120.0),
            ColumnSetting::new(Self::MODIFIED, 140.0),
        ]
    }

    /// The spec for an id, or none when this build cannot render it.
    ///
    /// An unknown id that [`ColumnId::looks_canonical`] accepts is a property this machine may
    /// simply have no handler for, so it gets a synthesized spec and renders blank. An unknown bare
    /// word came from a newer build and names a column this one does not have, so it is none and
    /// the layout drops it.
    pub fn try_get(id: &str) -> Option<ColumnSpec> {
        if id.is_empty() {
            return None;
        }
        let key = fold_upper(id);
        if let Some(spec) = BY_ID.get(&key) {
            return Some(spec.clone());
        }
        if !ColumnId::looks_canonical(id) {
            return None;
        }
        Some(
            CURATED_BY_ID
                .get(&key)
                .cloned()
                .unwrap_or_else(|| Self::spec_for_property(id, "")),
        )
    }

    /// The spec to sort by, degrading to Name rather than letting an unusable id reach the
    /// comparer: a saved sort order's "degrade, never fail" rule, one level down.
    pub fn sort_spec(id: Option<&str>) -> ColumnSpec {
        match id.and_then(Self::try_get) {
            Some(spec) if spec.sortable => spec,
            _ => BY_ID[&fold_upper(Self::NAME)].clone(),
        }
    }
}

static BUILT_INS: LazyLock<Vec<ColumnSpec>> = LazyLock::new(|| {
    vec![
        ColumnSpec::built_in(ColumnCatalog::NAME, "Name", 320.0),
        ColumnSpec::built_in(ColumnCatalog::RELATIVE_PATH, "Folder", 220.0),
        ColumnSpec::built_in(ColumnCatalog::MATCH, "Match", 420.0).not_sortable(),
        ColumnSpec::built_in(ColumnCatalog::COMPARE_STATUS, "Status", 110.0).not_sortable(),
        ColumnSpec::built_in(ColumnCatalog::SIZE, "Size", 110.0).right_aligned(),
        ColumnSpec::built_in(ColumnCatalog::TYPE, "Type", 120.0),
        ColumnSpec::built_in(ColumnCatalog::MODIFIED, "Modified", 140.0),
        ColumnSpec::built_in(ColumnCatalog::CREATED, "Created", 140.0),
        ColumnSpec::built_in(ColumnCatalog::ACCESSED, "Accessed", 140.0),
        ColumnSpec::built_in(ColumnCatalog::ATTRIBUTES, "Attributes", 90.0),
        ColumnSpec::built_in(ColumnCatalog::EXTENSION, "Extension", 90.0),
    ]
});

static SHADOWED: LazyLock<HashSet<String>> = LazyLock::new(|| {
    ColumnCatalog::SHADOWED_BY_BUILT_IN
        .iter()
        .map(|s| fold_upper(s))
        .collect()
});

// Composed from the preview pane's own lists rather than retyped: 38 canonical strings that
// would otherwise drift with nothing to notice. Media and Document share System.Title.
static CURATED: LazyLock<Vec<ColumnSpec>> = LazyLock::new(|| {
    let mut specs = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |canonicals: &[&str], group: &str| {
        for canonical in canonicals {
            if ColumnCatalog::is_shadowed_by_built_in(canonical)
                || !seen.insert(fold_upper(canonical))
            {
                continue;
            }
            specs.push(ColumnCatalog::spec_for_property(canonical, group));
        }
    };
    add(IMAGE_ORDER, "Image");
    add(MEDIA_ORDER, "Media");
    add(DOCUMENT_ORDER, "Document");
    add(
        &[
            "System.FileOwner",
            "System.Keywords",
            "System.Rating",
            "System.Comment",
        ],
        "File",
    );
    specs
});

static BY_ID: LazyLock<HashMap<String, ColumnSpec>> = LazyLock::new(|| {
    BUILT_INS
        .iter()
        .map(|s| (fold_upper(&s.id), s.clone()))
        .collect()
});

static CURATED_BY_ID: LazyLock<HashMap<String, ColumnSpec>> = LazyLock::new(|| {
    CURATED
        .iter()
        .map(|s| (fold_upper(&s.id), s.clone()))
        .collect()
});
