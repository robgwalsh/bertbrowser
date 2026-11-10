//! What the palette lists for what has been typed, and what it leaves out. **An empty box never lists
//! everything**: with nothing typed the list is what this person pinned, what they ran a moment ago,
//! a few commands that fit what is selected, and one row per category. **With something typed, the
//! best match is first**: how well the words matched, then whether the command can run right now,
//! then pins, recents and prominence. **A category narrows**: `view:` lists that category whole.
//! Spec: Core/Services/Commands/PaletteRules.cs.

use std::collections::{HashMap, HashSet};

use super::{CommandMatcher, CommandProminence, MatchRange};

/// One thing the palette can offer: a catalogue command, or something made from data (a workspace
/// to switch to, a bookmark to open). `unavailable` is why it cannot run right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteEntry {
    /// The command's id, or for a data entry one of its own (`workspace:Work`).
    pub id: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub gesture: String,
    pub aliases: Vec<String>,
    pub prominence: CommandProminence,
    pub unavailable: Option<String>,
    /// A second line of sorts (a path, a scope) shown beside the name.
    pub detail: String,
}

impl PaletteEntry {
    pub fn new(id: &str, name: &str, category: &str) -> PaletteEntry {
        PaletteEntry {
            id: id.to_string(),
            name: name.to_string(),
            category: category.to_string(),
            icon: None,
            gesture: String::new(),
            aliases: Vec::new(),
            prominence: CommandProminence::Standard,
            unavailable: None,
            detail: String::new(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.unavailable.is_none()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteRowKind {
    /// A label over the rows beneath it. Not choosable.
    Heading,
    /// Something to run.
    Entry,
    /// A category to narrow to. Choosing it puts its token in the box.
    Category,
}

/// One line of the palette's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRow {
    pub kind: PaletteRowKind,
    /// The heading, the entry's name, or the category's.
    pub text: String,
    pub entry: Option<PaletteEntry>,
    /// Where in `text` the typed words matched.
    pub highlights: Vec<MatchRange>,
    /// For a category row, what choosing it types: `view:`.
    pub token: String,
    /// For a category row, how many entries it holds.
    pub count: usize,
}

impl PaletteRow {
    fn heading(text: &str) -> PaletteRow {
        PaletteRow {
            kind: PaletteRowKind::Heading,
            text: text.to_string(),
            entry: None,
            highlights: Vec::new(),
            token: String::new(),
            count: 0,
        }
    }

    fn entry(entry: &PaletteEntry, highlights: Vec<MatchRange>) -> PaletteRow {
        PaletteRow {
            kind: PaletteRowKind::Entry,
            text: entry.name.clone(),
            entry: Some(entry.clone()),
            highlights,
            token: String::new(),
            count: 0,
        }
    }
}

/// What the palette knows about the person using it, beyond what they typed. Pins are persisted;
/// recents are this session's only.
#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    pub pins: Vec<String>,
    pub recents: Vec<String>,
    pub suggested: Vec<String>,
}

impl PaletteState {
    pub fn new(pins: &[&str], recents: &[&str], suggested: &[&str]) -> PaletteState {
        let owned = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
        PaletteState {
            pins: owned(pins),
            recents: owned(recents),
            suggested: owned(suggested),
        }
    }
}

pub struct PaletteRules;

impl PaletteRules {
    pub const MAX_RECENT: usize = 5;
    pub const MAX_SUGGESTED: usize = 8;
    pub const MAX_RESULTS: usize = 60;

    /// What a category row types, and what narrows to it: its name, lowercased, spaces as hyphens,
    /// then a colon.
    pub fn token(category: &str) -> String {
        format!("{}:", category.to_lowercase().replace(' ', "-"))
    }

    pub fn build(query: &str, entries: &[PaletteEntry], state: &PaletteState) -> Vec<PaletteRow> {
        let text = query.trim();
        let categories = Self::categories(entries);
        let (category, rest) = Self::split_category(text, &categories);
        if let Some(category) = category {
            let within: Vec<PaletteEntry> = entries
                .iter()
                .filter(|e| e.category == category)
                .cloned()
                .collect();
            return if rest.is_empty() {
                within
                    .iter()
                    .map(|e| PaletteRow::entry(e, Vec::new()))
                    .collect()
            } else {
                ranked(&rest, &within, state)
            };
        }
        if text.is_empty() {
            unasked(entries, state, &categories)
        } else {
            ranked(text, entries, state)
        }
    }

    /// The category a leading `name:` names, and what follows it. At least two letters, so a drive
    /// (`c:`) is never mistaken for one; and it has to be the start of exactly one category.
    pub fn split_category(text: &str, categories: &[String]) -> (Option<String>, String) {
        let Some(colon) = text.find(':') else {
            return (None, text.to_string());
        };
        if colon < 2 {
            return (None, text.to_string());
        }
        let typed = text[..colon].trim().replace(' ', "-");
        if typed.chars().count() < 2 || typed.chars().any(|c| !c.is_alphabetic() && c != '-') {
            return (None, text.to_string());
        }
        let typed_lower = typed.to_lowercase();
        let matches: Vec<&String> = categories
            .iter()
            .filter(|c| Self::token(c).starts_with(&typed_lower))
            .collect();
        // An exact name wins over being a prefix of several.
        let exact = matches
            .iter()
            .find(|c| Self::token(c) == format!("{typed_lower}:"))
            .copied();
        let chosen = exact.or_else(|| {
            if matches.len() == 1 {
                Some(matches[0])
            } else {
                None
            }
        });
        match chosen {
            None => (None, text.to_string()),
            Some(c) => (Some(c.clone()), text[colon + 1..].trim().to_string()),
        }
    }

    /// The categories present, in the order they first appear among the entries.
    pub fn categories(entries: &[PaletteEntry]) -> Vec<String> {
        let mut seen = HashSet::new();
        entries
            .iter()
            .filter(|e| seen.insert(e.category.as_str()))
            .map(|e| e.category.clone())
            .collect()
    }
}

fn unasked(
    entries: &[PaletteEntry],
    state: &PaletteState,
    categories: &[String],
) -> Vec<PaletteRow> {
    let mut by_id: HashMap<&str, &PaletteEntry> = HashMap::new();
    for entry in entries {
        by_id.entry(entry.id.as_str()).or_insert(entry);
    }
    let mut rows = Vec::new();
    let mut shown: HashSet<String> = HashSet::new();
    let mut section = |heading: &str,
                       ids: &[String],
                       limit: usize,
                       available_only: bool,
                       rows: &mut Vec<PaletteRow>| {
        let picked: Vec<&PaletteEntry> = ids
            .iter()
            .filter(|id| !shown.contains(id.as_str()))
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|e| !available_only || e.is_available())
            .take(limit)
            .collect();
        if picked.is_empty() {
            return;
        }
        rows.push(PaletteRow::heading(heading));
        for entry in picked {
            rows.push(PaletteRow::entry(entry, Vec::new()));
            shown.insert(entry.id.clone());
        }
    };
    // A pin is shown even when it cannot run: it was put there on purpose.
    section("Pinned", &state.pins, usize::MAX, false, &mut rows);
    // A recent that cannot run from here is noise, where a pin that cannot is a fixed point.
    section(
        "Recent",
        &state.recents,
        PaletteRules::MAX_RECENT,
        true,
        &mut rows,
    );
    // A suggestion that cannot run is not a suggestion.
    section(
        "Suggested",
        &state.suggested,
        PaletteRules::MAX_SUGGESTED,
        true,
        &mut rows,
    );

    rows.push(PaletteRow::heading("All commands"));
    for category in categories {
        rows.push(PaletteRow {
            kind: PaletteRowKind::Category,
            text: category.clone(),
            entry: None,
            highlights: Vec::new(),
            token: PaletteRules::token(category),
            count: entries.iter().filter(|e| &e.category == category).count(),
        });
    }
    rows
}

fn ranked(text: &str, entries: &[PaletteEntry], state: &PaletteState) -> Vec<PaletteRow> {
    let pins: HashSet<&str> = state.pins.iter().map(String::as_str).collect();
    let recents: HashSet<&str> = state.recents.iter().map(String::as_str).collect();
    let mut scored: Vec<(usize, &PaletteEntry, super::CommandMatch)> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let aliases: Vec<&str> = entry.aliases.iter().map(String::as_str).collect();
            (
                index,
                entry,
                CommandMatcher::matches(
                    text,
                    &entry.name,
                    &aliases,
                    &entry.category,
                    &entry.gesture,
                ),
            )
        })
        .filter(|(_, _, m)| m.is_match())
        .collect();
    scored.sort_by_key(|(index, entry, m)| {
        (
            std::cmp::Reverse(m.tier),
            std::cmp::Reverse(entry.is_available()),
            std::cmp::Reverse(m.score + lift(entry, &pins, &recents)),
            *index,
        )
    });
    scored
        .into_iter()
        .take(PaletteRules::MAX_RESULTS)
        .map(|(_, e, m)| PaletteRow::entry(e, m.highlights))
        .collect()
}

/// What pins, recents and prominence add to a match's score: enough to order equals, never enough to
/// lift a worse match over a better one of the same tier by much.
fn lift(entry: &PaletteEntry, pins: &HashSet<&str>, recents: &HashSet<&str>) -> i32 {
    let mut lift = match entry.prominence {
        CommandProminence::Essential => 6,
        CommandProminence::Rare => -6,
        CommandProminence::Standard => 0,
    };
    if pins.contains(entry.id.as_str()) {
        lift += 12;
    }
    if recents.contains(entry.id.as_str()) {
        lift += 8;
    }
    lift
}

/// What an empty palette suggests, given where the window is. Curated rather than computed: nothing
/// here is counted or remembered.
pub struct PaletteSuggestions;

impl PaletteSuggestions {
    /// With something selected, the things done to a selection.
    const FOR_SELECTION: &'static [&'static str] = &[
        "file.rename",
        "pane.copy-to-other",
        "pane.move-to-other",
        "edit.copy-path",
        "file.compress",
        "file.extract-here",
        "file.properties",
        "file.checksum",
        "file.compare-files",
        "file.copy-to",
        "file.move-to",
        "file.open-terminal",
        "select.invert",
    ];
    /// With nothing selected, getting somewhere and seeing it differently.
    const FOR_FOLDER: &'static [&'static str] = &[
        "nav.address-bar",
        "tab.new",
        "pane.split-right",
        "select.pattern",
        "view.flat",
        "view.preview",
        "view.hidden",
        "file.new-folder",
        "search.pc",
        "tools.disk-usage",
        "tools.duplicates",
        "file.open-terminal",
        "view.toggle-thumbnails",
        "tools.compare",
    ];
    /// On the settings page, where nothing about the folders can run.
    const FOR_SETTINGS: &'static [&'static str] = &[
        "app.settings",
        "app.keyboard",
        "app.customise-theme",
        "app.match-windows-theme",
        "search.syntax",
    ];

    pub fn for_state(selected: usize, settings_open: bool) -> &'static [&'static str] {
        if settings_open {
            Self::FOR_SETTINGS
        } else if selected > 0 {
            Self::FOR_SELECTION
        } else {
            Self::FOR_FOLDER
        }
    }

    /// Every id any list above names, so a test can hold them to the catalogue.
    pub fn all() -> Vec<&'static str> {
        let mut seen = HashSet::new();
        Self::FOR_SELECTION
            .iter()
            .chain(Self::FOR_FOLDER)
            .chain(Self::FOR_SETTINGS)
            .copied()
            .filter(|id| seen.insert(*id))
            .collect()
    }
}
