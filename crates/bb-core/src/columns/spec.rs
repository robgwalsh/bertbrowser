//! What a column *is*: immutable catalogue data, the same on every machine and in every tab.
//! Spec: Core/Services/Columns/ColumnSpec.cs.

use std::fmt;

/// Where a column's value comes from. Lives on [`ColumnSpec`], which is catalogue data and never
/// serialized; it must not move onto `ColumnSetting`, where a reordering of the members would
/// silently change what an old settings file means.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColumnKind {
    /// Answered from the row itself — no shell call, no I/O.
    BuiltIn,
    /// Read from the Windows property system, keyed by canonical name.
    ShellProperty,
}

/// A shell property's identity as the property system keys it: a format id and a property id.
/// Kept as data — the shell layer fills it in from a canonical name (`PSGetPropertyKeyFromName`)
/// and reads the store; nothing in Core resolves one. The catalogue itself is keyed by canonical
/// name, since that is what settings persist and what two machines agree on.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PropertyKey {
    /// The format id as a braced GUID string, `{14B81DA1-0135-4D31-96D9-6CBFC9671A99}`.
    pub fmtid: String,
    pub pid: u32,
}

impl fmt::Display for PropertyKey {
    /// The property system's own textual form, `{fmtid} pid`, which it parses back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.fmtid, self.pid)
    }
}

/// What a column is. `id` is the identity persisted in settings: a built-in's id is a bare word
/// (`Modified`); a shell property's id **is its canonical name** (`System.Photo.DateTaken`). The
/// two spaces cannot collide because a canonical name always contains a dot and a built-in id never
/// does — which is what lets the layout rules tell "a built-in from a newer version, drop it" from
/// "a property this machine may not have, keep it and render blank".
#[derive(Clone, PartialEq, Debug)]
pub struct ColumnSpec {
    pub id: String,
    /// The column header. For a built-in this is the header. For a shell property it is only a
    /// *fallback*: the real one is the localised name from `IPropertyDescription::GetDisplayName`,
    /// resolved once when the column is added — hard-coding "Dimensions" would read as gibberish
    /// on a German Windows.
    pub header: String,
    pub kind: ColumnKind,
    pub default_width: f64,
    pub right_aligned: bool,
    /// False only for Match. Sorting a result set by the line number a needle was found on means
    /// nothing across different files.
    pub sortable: bool,
    /// The header-menu group a curated property sits under; empty for a built-in or an uncurated
    /// property.
    pub group: String,
}

impl ColumnSpec {
    pub fn built_in(id: &str, header: &str, default_width: f64) -> ColumnSpec {
        ColumnSpec {
            id: id.to_string(),
            header: header.to_string(),
            kind: ColumnKind::BuiltIn,
            default_width,
            right_aligned: false,
            sortable: true,
            group: String::new(),
        }
    }

    pub fn shell_property(
        canonical: &str,
        header: &str,
        default_width: f64,
        group: &str,
    ) -> ColumnSpec {
        ColumnSpec {
            id: canonical.to_string(),
            header: header.to_string(),
            kind: ColumnKind::ShellProperty,
            default_width,
            right_aligned: false,
            sortable: true,
            group: group.to_string(),
        }
    }

    pub fn right_aligned(mut self) -> ColumnSpec {
        self.right_aligned = true;
        self
    }

    pub fn not_sortable(mut self) -> ColumnSpec {
        self.sortable = false;
        self
    }

    /// The canonical property name a shell column reads, or none for a built-in.
    pub fn canonical_name(&self) -> Option<&str> {
        match self.kind {
            ColumnKind::ShellProperty => Some(&self.id),
            ColumnKind::BuiltIn => None,
        }
    }
}

/// What the view builds: a spec, the width it should get, and whether the app put it there rather
/// than the user.
#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedColumn {
    pub spec: ColumnSpec,
    pub width: f64,
    /// True for Folder, Match and Status, which follow the list's mode rather than anyone's choice
    /// and are never persisted; `ColumnLayoutRules::capture_order` drops them on the way back.
    pub injected: bool,
}

impl ResolvedColumn {
    pub fn id(&self) -> &str {
        &self.spec.id
    }
}
