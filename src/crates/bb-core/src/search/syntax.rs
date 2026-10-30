//! The filter keys the query language understands, and the help text describing them. Parser and UI
//! read the same table, so the popover cannot advertise a key the parser does not implement. **An
//! unrecognised key is a name term, not an error**: `C:\Users` pasted into the box has to keep
//! meaning a search for that text. Spec: Core/Services/Search/SearchSyntax.cs.

pub struct SearchSyntax;

/// One line of the search syntax help.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSyntaxEntry {
    pub example: &'static str,
    pub meaning: &'static str,
}

/// A group of related help lines, with a heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSyntaxSection {
    pub title: &'static str,
    pub blurb: &'static str,
    pub entries: &'static [SearchSyntaxEntry],
}

const fn e(example: &'static str, meaning: &'static str) -> SearchSyntaxEntry {
    SearchSyntaxEntry { example, meaning }
}

impl SearchSyntax {
    // Recognised keys, upper-cased.
    pub const EXTENSION: &'static str = "EXT";
    pub const PATH: &'static str = "PATH";
    pub const SIZE: &'static str = "SIZE";
    pub const MODIFIED: &'static str = "DM";
    pub const CREATED: &'static str = "DC";
    pub const IS: &'static str = "IS";
    pub const REGEX: &'static str = "RE";
    pub const NAME: &'static str = "NAME";
    pub const IN: &'static str = "IN";
    pub const CONTENT: &'static str = "CONTENT";

    /// Resolves a typed (upper-cased) key to its canonical form, or None when it is not a key at all.
    pub fn resolve(key: &str) -> Option<&'static str> {
        Some(match key {
            "EXT" | "EXTENSION" => Self::EXTENSION,
            "PATH" => Self::PATH,
            "SIZE" => Self::SIZE,
            "DM" | "MODIFIED" | "DATEMODIFIED" => Self::MODIFIED,
            "DC" | "CREATED" | "DATECREATED" => Self::CREATED,
            "IS" => Self::IS,
            "RE" | "REGEX" => Self::REGEX,
            "NAME" => Self::NAME,
            "IN" => Self::IN,
            "CONTENT" | "CONTENTS" => Self::CONTENT,
            _ => return None,
        })
    }

    /// Why a key cannot be used, or None when it is fine or is not a known key. `da:` is refused
    /// because Windows 10 and 11 ship with last-access updates disabled: the timestamp is on disk,
    /// frozen, so a filter over it would answer confidently and wrongly.
    pub fn unsupported_reason(key: &str) -> Option<&'static str> {
        match key {
            "DA" | "DATEACCESSED" => Some(
                "Windows stops updating the accessed date by default, so it would answer wrongly \u{2014} use dm: or dc:",
            ),
            _ => None,
        }
    }

    /// The help, grouped, in the order it should be shown.
    pub const SECTIONS: &'static [SearchSyntaxSection] = &[
        SearchSyntaxSection {
            title: "Words",
            blurb: "Type anything. Several words all have to match, in any order.",
            entries: &[
                e("report 2026", "names containing both words"),
                e("\"annual report\"", "an exact phrase, spaces and all"),
                e("*.log", "* matches any run, ? matches one character"),
            ],
        },
        SearchSyntaxSection {
            title: "Filters",
            blurb: "Narrow by something other than the name.",
            entries: &[
                e("ext:jpg;png", "one of these extensions"),
                e("size:>100mb", "also <, >=, <=, =, 1mb..2gb and empty"),
                e("dm:today", "also yesterday, thisweek, last7days, 2026-08"),
                e(
                    "dc:2026-08",
                    "created instead of modified \u{2014} same forms as dm:",
                ),
                e(
                    "path:projects",
                    "somewhere in the folder path, not just the name",
                ),
                e("is:dir", "folders only \u{2014} also is:file and is:hidden"),
                e("is:readonly", "also system, archived, link and compressed"),
                e("re:^IMG_\\d+", "a regular expression over the name"),
                e(
                    "in:archives",
                    "look inside zips and 7zs too \u{2014} names, not contents",
                ),
                e(
                    "content:todo",
                    "text inside the file \u{2014} also content:\"a phrase\"",
                ),
            ],
        },
        SearchSyntaxSection {
            title: "Combining",
            blurb: "Words sit side by side to mean AND; the rest is spelled out.",
            entries: &[
                e("!tmp", "exclude \u{2014} NOT works the same way"),
                e("draft OR final", "either one; OR must be uppercase"),
                e("(a OR b) ext:txt", "brackets group"),
            ],
        },
        SearchSyntaxSection {
            title: "Worth knowing",
            blurb: "Where the answers come from.",
            entries: &[
                e(
                    "size:, dm: and dc:",
                    "need a drive the indexer read in full",
                ),
                e(
                    "da:",
                    "is refused \u{2014} Windows stops updating the accessed date",
                ),
                e(
                    "content:",
                    "opens each file, so narrow it with ext: or a folder",
                ),
                e(
                    "in:archives",
                    "opens each archive, so it is slower and opt-in",
                ),
            ],
        },
    ];

    /// The same help flattened, for the tooltip. Derived, so the two cannot disagree.
    pub fn entries() -> impl Iterator<Item = &'static SearchSyntaxEntry> {
        Self::SECTIONS.iter().flat_map(|s| s.entries.iter())
    }
}
