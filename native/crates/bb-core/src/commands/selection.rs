//! "Select by pattern": which rows of the listing on screen a typed pattern picks. The pattern is the
//! search box's own language, so there is one syntax to learn. What differs is the subject: this
//! judges rows already listed and reads nothing, so a term that needs a file opened or names another
//! place to look is refused rather than answered wrongly. Spec: Core/Services/Commands/SelectionPattern.cs.

use bb_paths::fold_upper;

use crate::FileTime;
use crate::attributes;
use crate::search::{SearchCandidate, SearchGrammar, SearchMatch};

/// One row of a listing, as "select by pattern" needs to see it.
#[derive(Clone, Debug)]
pub struct SelectionRow {
    pub name: String,
    pub full_path: String,
    pub is_directory: bool,
    pub size_bytes: i64,
    pub modified_utc: FileTime,
    pub created_utc: FileTime,
    pub attributes: u32,
}

/// Which rows a pattern picks (by position in the list that was asked about), or why it could not be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionMatch {
    pub indexes: Vec<usize>,
    pub problem: Option<String>,
}

pub struct SelectionPattern;

impl SelectionPattern {
    pub fn matches(pattern: &str, rows: &[SelectionRow]) -> SelectionMatch {
        // No floor on how broad the pattern may be: it is judged against the rows on screen.
        let parse = SearchGrammar::parse(pattern, false);
        if let Some(problem) = parse.problem {
            return SelectionMatch {
                indexes: Vec::new(),
                problem: Some(problem),
            };
        }
        let Some(query) = parse.query else {
            return SelectionMatch {
                indexes: Vec::new(),
                problem: None,
            };
        };
        if query.needs_content() {
            return SelectionMatch {
                indexes: Vec::new(),
                problem: Some(
                    "content: reads files, which selecting does not do. Search for it instead."
                        .into(),
                ),
            };
        }
        if query.wants_archives() {
            return SelectionMatch {
                indexes: Vec::new(),
                problem: Some(
                    "in:archives looks inside containers, which selecting does not do.".into(),
                ),
            };
        }
        let mut matched = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let name_key = fold_upper(&row.name);
            let path_key = fold_upper(&row.full_path);
            let candidate = SearchCandidate {
                name_key: &name_key,
                path_key: &path_key,
                is_directory: row.is_directory,
                size_bytes: row.size_bytes,
                modified_utc: row.modified_utc,
                // A hidden row is on screen because hidden items are being shown, so it is as
                // selectable as its neighbours; is:hidden still tells them apart by the mask.
                hidden: row.attributes & attributes::HIDDEN != 0,
                attributes: row.attributes,
                created_utc: row.created_utc,
                content: None,
            };
            if query.evaluate(&candidate) == SearchMatch::Yes {
                matched.push(i);
            }
        }
        SelectionMatch {
            indexes: matched,
            problem: None,
        }
    }
}
