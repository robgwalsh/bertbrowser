//! What the "Add column" popup shows: the columns that could be added, grouped and filtered.
//!
//! Pure, so the one hard part of that popup — what is curated, what has already been added, what
//! the machine reported and what the search matched — is settled where tests can reach it rather
//! than inside a text-changed handler. The popup only ever *adds*. Removing is the list's own job,
//! which is why anything already in the layout is simply absent here rather than shown ticked.
//! Spec: Core/Services/Columns/ColumnCandidates.cs.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use bb_paths::fold_upper;
use windows_sys::Win32::Globalization::{
    CompareStringEx, FIND_FROMSTART, FindNLSStringEx, LINGUISTIC_IGNORECASE,
};

use super::{ColumnCatalog, ColumnId, ColumnKind, ColumnLayoutRules, ColumnSetting, ColumnSpec};

/// Which of the add popup's two lists a candidate belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColumnCandidateKind {
    /// The built-ins and [`ColumnCatalog::curated`] — what nearly everyone wants, answerable
    /// without reading the property system at all.
    Common,
    /// Everything else this machine reported.
    All,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ColumnCandidate {
    pub id: String,
    /// The label to show: the shell's localised name where there is one, and the catalogue's
    /// fallback where there is not.
    pub header: String,
    /// The canonical name, shown dimmed beside the label, or empty for a built-in. It is what
    /// lands in the settings file and the only way to tell two similarly-named properties apart.
    pub detail: String,
    pub kind: ColumnKind,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ColumnCandidateGroup {
    pub kind: ColumnCandidateKind,
    /// The section heading. A group with nothing in it is dropped rather than rendered as a bare
    /// heading.
    pub title: String,
    pub items: Vec<ColumnCandidate>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ColumnCandidates {
    pub groups: Vec<ColumnCandidateGroup>,
    /// The property system has not been enumerated yet. Common is still worth showing — it needs
    /// no machine — so the popup opens filled rather than blank.
    pub is_loading: bool,
    /// The layout is already at [`ColumnLayoutRules::MAX_COLUMNS`], so nothing can be added.
    /// Distinct from a search that matched nothing, and says so differently.
    pub is_full: bool,
}

impl ColumnCandidates {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// `layout` is the columns already chosen; none means never configured, read the same way
    /// [`ColumnLayoutRules::normalize`] reads it. `machine` is what the shell's property
    /// enumeration returned — canonical name and localised display name — empty while it is still
    /// being read. `properties_loaded` is false until that enumeration has come back: an empty
    /// `machine` is otherwise ambiguous between "still reading" and "this PC reported none", and
    /// the two need different words on screen.
    pub fn build<S: AsRef<str>, T: AsRef<str>>(
        layout: Option<&[ColumnSetting]>,
        machine: &[(S, T)],
        search: &str,
        properties_loaded: bool,
    ) -> ColumnCandidates {
        let current = ColumnLayoutRules::normalize(layout);

        // Doubles as the not-already-offered set: every list below adds to it as it goes, which is
        // what keeps a curated property from appearing again under All properties.
        let mut taken: HashSet<String> = current.iter().map(|c| fold_upper(&c.id)).collect();
        let full = current.len() >= ColumnLayoutRules::MAX_COLUMNS;

        let needle = search.trim();
        let mut groups = Vec::with_capacity(2);

        // The shell's own labels beat the synthesized ones the catalogue falls back to: on a German
        // Windows "Aufnahmedatum" is the name of that property and "DateTaken" is not.
        let mut localised: HashMap<String, &str> = HashMap::new();
        for (canonical, display) in machine {
            localised
                .entry(fold_upper(canonical.as_ref()))
                .or_insert(display.as_ref());
        }

        if !full {
            let mut common = Vec::new();
            for spec in ColumnCatalog::built_ins()
                .iter()
                .chain(ColumnCatalog::curated())
            {
                if ColumnCatalog::is_injected(&spec.id) || !taken.insert(fold_upper(&spec.id)) {
                    continue;
                }
                common.push(candidate(spec, &localised));
            }

            add(
                ColumnCandidateKind::Common,
                "Common",
                common,
                needle,
                &mut groups,
            );

            let mut rest = Vec::new();
            for (canonical, display) in machine {
                let canonical = canonical.as_ref();
                if ColumnCatalog::is_shadowed_by_built_in(canonical)
                    || !ColumnId::looks_canonical(canonical)
                    || !taken.insert(fold_upper(canonical))
                {
                    continue;
                }
                rest.push(ColumnCandidate {
                    id: canonical.to_string(),
                    header: display.as_ref().to_string(),
                    detail: canonical.to_string(),
                    kind: ColumnKind::ShellProperty,
                });
            }

            rest.sort_by(|a, b| culture_cmp_ignore_case(&a.header, &b.header));
            add(
                ColumnCandidateKind::All,
                "All properties",
                rest,
                needle,
                &mut groups,
            );
        }

        ColumnCandidates {
            groups,
            is_loading: !properties_loaded && !full,
            is_full: full,
        }
    }
}

fn candidate(spec: &ColumnSpec, localised: &HashMap<String, &str>) -> ColumnCandidate {
    if spec.kind == ColumnKind::BuiltIn {
        return ColumnCandidate {
            id: spec.id.clone(),
            header: spec.header.clone(),
            detail: String::new(),
            kind: spec.kind,
        };
    }

    let header = localised
        .get(&fold_upper(&spec.id))
        .map_or_else(|| spec.header.clone(), |display| display.to_string());
    ColumnCandidate {
        id: spec.id.clone(),
        header,
        detail: spec.id.clone(),
        kind: spec.kind,
    }
}

/// Filters a group and adds it, unless the search emptied it — a heading over nothing reads as a
/// rendering fault.
fn add(
    kind: ColumnCandidateKind,
    title: &str,
    items: Vec<ColumnCandidate>,
    needle: &str,
    groups: &mut Vec<ColumnCandidateGroup>,
) {
    let matched: Vec<ColumnCandidate> = if needle.is_empty() {
        items
    } else {
        items.into_iter().filter(|c| matches(c, needle)).collect()
    };
    if !matched.is_empty() {
        groups.push(ColumnCandidateGroup {
            kind,
            title: title.to_string(),
            items: matched,
        });
    }
}

/// The canonical name is searchable as well as the label, because it is on the row and because it
/// is the half a person is likely to have seen in the settings file. The label is matched the way
/// the user's language reads it; the canonical name is ASCII and matched ordinally.
fn matches(candidate: &ColumnCandidate, needle: &str) -> bool {
    culture_contains_ignore_case(&candidate.header, needle)
        || fold_upper(&candidate.detail).contains(&fold_upper(needle))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The user's culture, case ignored — how a list of localised labels is expected to read.
fn culture_cmp_ignore_case(a: &str, b: &str) -> Ordering {
    let (a, b) = (wide(a), wide(b));
    // SAFETY: both buffers are valid for their stated lengths; a null locale name means the user
    // default; the version, reserved and sort-handle arguments are allowed to be null/zero.
    let r = unsafe {
        CompareStringEx(
            std::ptr::null(),
            LINGUISTIC_IGNORECASE,
            a.as_ptr(),
            a.len() as i32,
            b.as_ptr(),
            b.len() as i32,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    // CSTR_LESS_THAN = 1, CSTR_EQUAL = 2, CSTR_GREATER_THAN = 3; 0 is a failure, read as equal.
    match r {
        1 => Ordering::Less,
        3 => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn culture_contains_ignore_case(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let (hay, needle) = (wide(hay), wide(needle));
    // SAFETY: as above; the found-length out pointer may be null.
    let at = unsafe {
        FindNLSStringEx(
            std::ptr::null(),
            FIND_FROMSTART | LINGUISTIC_IGNORECASE,
            hay.as_ptr(),
            hay.len() as i32,
            needle.as_ptr(),
            needle.len() as i32,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    at >= 0
}
