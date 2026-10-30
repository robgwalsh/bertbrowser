//! One node of a parsed query: a leaf term, or an AND/OR/NOT over other nodes. **Every term answers
//! both questions**, "does this entry match?" and "how do I say that in SQL?", so a new filter cannot
//! be added with only one side wired up. **`matches` is the definition; the SQL is only an
//! optimisation.** The repository re-applies `matches` to every row it reads back, so SQL that is
//! too wide costs a slower scan and nothing else; too narrow drops rows and the agreement test goes
//! red. Terms that cannot be expressed at all emit `1`. Spec: Core/Services/Search/SearchNode.cs, SearchTerms.cs.

use super::{GlobText, SearchCandidate, SearchMatch, SqlPredicateBuilder};
use crate::FileTime;

/// Which timestamp a date term is about. Accessed is deliberately absent: Windows ships with
/// last-access updates disabled, so the value on disk is frozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateField {
    Modified,
    Created,
}

pub enum SearchNode {
    And(Vec<SearchNode>),
    Or(Vec<SearchNode>),
    Not(Box<SearchNode>),
    /// A substring of the entry name with `*` and `?` honoured: the bare-word term. `term` is
    /// upper-cased; `literal` is a quoted phrase, where wildcards lose their meaning.
    Name {
        term: String,
        literal: bool,
        pattern: String,
        glob: String,
    },
    /// A substring of the entry's full path.
    Path {
        term: String,
        pattern: String,
        glob: String,
    },
    /// One of a set of extensions, upper-cased, without the dot.
    Extension(Vec<String>),
    /// A byte-length range, half-open `[lo, hi)`. **Only files**: a directory's indexed length is 0.
    Size {
        lo: Option<i64>,
        hi: Option<i64>,
    },
    /// A timestamp range, half-open `[lo, hi)` in UTC, over one field.
    Date {
        lo: Option<FileTime>,
        hi: Option<FileTime>,
        field: DateField,
    },
    /// Directory or file.
    Kind {
        is_directory: bool,
    },
    /// Hidden entries (the effective flag), and the reason `wants_hidden` exists.
    Hidden,
    /// One Win32 attribute bit of the entry's own mask.
    Attribute(u32),
    /// `in:archives`: a scope rather than a predicate. `matches` is always yes and the SQL `1`.
    InArchives,
    /// A regular expression over the entry name. The one term with no SQL at all.
    Regex(fancy_regex::Regex),
    /// Text somewhere inside the file: the one term that reads the disk. The needle keeps the case
    /// it was typed in and is compared ignoring case.
    Content(String),
}

/// `*` + the term + `*`: the term as typed when `plain`, else GLOB-escaped (see `GlobText::escape`).
fn star_wrapped(term: &str, plain: bool, keep_wildcards: bool) -> String {
    let mut out = String::with_capacity(term.len() + 10);
    out.push('*');
    if plain {
        out.push_str(term);
    } else {
        GlobText::escape_into(&mut out, term, keep_wildcards);
    }
    out.push('*');
    out
}

impl SearchNode {
    pub fn name(term: &str, literal: bool) -> SearchNode {
        Self::name_owned(term.to_string(), literal)
    }

    /// [`name`](Self::name) for a caller that already owns the folded term.
    pub fn name_owned(term: String, literal: bool) -> SearchNode {
        let pattern = star_wrapped(&term, !literal, false);
        let glob = star_wrapped(&term, false, !literal);
        SearchNode::Name {
            term,
            literal,
            pattern,
            glob,
        }
    }

    pub fn path(term: &str) -> SearchNode {
        let glob = star_wrapped(term, false, true);
        let mut pattern = String::with_capacity(term.len() + 2);
        pattern.push('*');
        pattern.push_str(term);
        pattern.push('*');
        SearchNode::Path {
            term: term.to_string(),
            pattern,
            glob,
        }
    }

    /// Whether `candidate` satisfies this node. The definition.
    pub fn matches(&self, c: &SearchCandidate<'_>) -> SearchMatch {
        match self {
            // One settled No refuses the whole conjunction without reading anything, which is what
            // makes `is:dir content:x` cost nothing.
            SearchNode::And(children) => {
                let mut undecided = false;
                for child in children {
                    match child.matches(c) {
                        SearchMatch::No => return SearchMatch::No,
                        SearchMatch::NeedsContent => undecided = true,
                        SearchMatch::Yes => {}
                    }
                }
                if undecided {
                    SearchMatch::NeedsContent
                } else {
                    SearchMatch::Yes
                }
            }
            // One settled Yes satisfies the whole disjunction without reading anything.
            SearchNode::Or(children) => {
                let mut undecided = false;
                for child in children {
                    match child.matches(c) {
                        SearchMatch::Yes => return SearchMatch::Yes,
                        SearchMatch::NeedsContent => undecided = true,
                        SearchMatch::No => {}
                    }
                }
                if undecided {
                    SearchMatch::NeedsContent
                } else {
                    SearchMatch::No
                }
            }
            // Negation flips a settled answer and leaves an unsettled one alone.
            SearchNode::Not(child) => match child.matches(c) {
                SearchMatch::Yes => SearchMatch::No,
                SearchMatch::No => SearchMatch::Yes,
                SearchMatch::NeedsContent => SearchMatch::NeedsContent,
            },
            SearchNode::Name {
                term,
                literal,
                pattern,
                ..
            } => SearchMatch::verdict(if *literal {
                c.name_key.contains(term.as_str())
            } else {
                GlobText::wildcard_match(c.name_key, pattern)
            }),
            SearchNode::Path { pattern, .. } => {
                SearchMatch::verdict(GlobText::wildcard_match(c.path_key, pattern))
            }
            SearchNode::Extension(extensions) => {
                let name = c.name_key;
                for ext in extensions {
                    // A file *called* ".jpg" has no extension, it has a dotted name: require at
                    // least one character before the dot.
                    if name.len() > ext.len() + 1
                        && name.ends_with(ext.as_str())
                        && name.as_bytes()[name.len() - ext.len() - 1] == b'.'
                    {
                        return SearchMatch::Yes;
                    }
                }
                SearchMatch::No
            }
            SearchNode::Size { lo, hi } => {
                if c.is_directory {
                    return SearchMatch::No;
                }
                if lo.is_some_and(|lo| c.size_bytes < lo) || hi.is_some_and(|hi| c.size_bytes >= hi)
                {
                    return SearchMatch::No;
                }
                SearchMatch::Yes
            }
            SearchNode::Date { lo, hi, field } => {
                let value = match field {
                    DateField::Created => c.created_utc,
                    DateField::Modified => c.modified_utc,
                };
                // A row with no timestamp must not satisfy a date filter, least of all an
                // open-ended one.
                if value.is_unknown() {
                    return SearchMatch::No;
                }
                if lo.is_some_and(|lo| value < lo) || hi.is_some_and(|hi| value >= hi) {
                    return SearchMatch::No;
                }
                SearchMatch::Yes
            }
            SearchNode::Kind { is_directory } => {
                SearchMatch::verdict(c.is_directory == *is_directory)
            }
            SearchNode::Hidden => SearchMatch::verdict(c.hidden),
            SearchNode::Attribute(flag) => SearchMatch::verdict(c.attributes & flag != 0),
            SearchNode::InArchives => SearchMatch::Yes,
            // A pattern that blows its budget on one name must not abort the whole search.
            SearchNode::Regex(regex) => {
                SearchMatch::verdict(regex.is_match(c.name_key).unwrap_or(false))
            }
            // A directory is settled No: it has no contents to search. Unread content is undecided.
            SearchNode::Content(needle) => {
                if c.is_directory {
                    return SearchMatch::No;
                }
                match c.content {
                    None => SearchMatch::NeedsContent,
                    Some(content) => SearchMatch::verdict(content.index_of(needle).is_some()),
                }
            }
        }
    }

    /// Writes a WHERE fragment that matches *at least* everything `matches` does. A node that cannot
    /// be expressed marks the builder incomplete and emits `1`.
    pub fn write_sql(&self, b: &mut SqlPredicateBuilder) {
        match self {
            SearchNode::And(children) => {
                b.append("(");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        b.append(" AND ");
                    }
                    child.write_sql(b);
                }
                b.append(")");
            }
            SearchNode::Or(children) => {
                b.append("(");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        b.append(" OR ");
                    }
                    child.write_sql(b);
                }
                b.append(")");
            }
            // A superset cannot be negated: negating a wider set gives a narrower one, which would
            // drop rows that really match. So when the child is inexact this emits `1` and leans on
            // the re-check. Without this `!re:foo` compiles to `NOT 1` and returns nothing.
            SearchNode::Not(child) => {
                if !child.sql_complete() {
                    b.mark_incomplete();
                    b.append("1");
                    return;
                }
                b.append("NOT ");
                child.write_sql(b);
            }
            SearchNode::Name { glob, .. } => {
                b.append("name_key GLOB ").append_parameter(glob.clone());
            }
            SearchNode::Path { glob, .. } => {
                b.append("path_key GLOB ").append_parameter(glob.clone());
            }
            SearchNode::Extension(extensions) => {
                b.append("(");
                for (i, ext) in extensions.iter().enumerate() {
                    if i > 0 {
                        b.append(" OR ");
                    }
                    b.append("name_key GLOB ")
                        .append_parameter(format!("?*.{}", GlobText::escape(ext, false)));
                }
                b.append(")");
            }
            SearchNode::Size { lo, hi } => {
                b.append("(is_dir = 0");
                if let Some(lo) = lo {
                    b.append(" AND size_bytes >= ").append_parameter(*lo);
                }
                if let Some(hi) = hi {
                    b.append(" AND size_bytes < ").append_parameter(*hi);
                }
                b.append(")");
            }
            SearchNode::Date { lo, hi, field } => {
                let column = match field {
                    DateField::Created => "created_utc",
                    DateField::Modified => "modified_utc",
                };
                // Unknown is stored as 0, which the floor of 1 excludes, so a row with no
                // timestamp satisfies no filter rather than every open-ended one.
                let floor = lo.map_or(1, |lo| lo.0.max(1));
                b.append("(")
                    .append(column)
                    .append(" >= ")
                    .append_parameter(floor);
                if let Some(hi) = hi {
                    b.append(" AND ")
                        .append(column)
                        .append(" < ")
                        .append_parameter(hi.0);
                }
                b.append(")");
            }
            SearchNode::Kind { is_directory } => {
                b.append("is_dir = ")
                    .append_parameter(i64::from(*is_directory));
            }
            SearchNode::Hidden => {
                b.append("hidden = 1");
            }
            SearchNode::Attribute(flag) => {
                b.append("(attributes & ")
                    .append_parameter(i64::from(*flag))
                    .append(") != 0");
            }
            SearchNode::InArchives => {
                b.append("1");
            }
            SearchNode::Regex(_) | SearchNode::Content(_) => {
                b.mark_incomplete();
                b.append("1");
            }
        }
    }

    /// Whether this node's SQL is exact rather than a superset.
    pub fn sql_complete(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().all(SearchNode::sql_complete),
            SearchNode::Not(c) => c.sql_complete(),
            SearchNode::InArchives | SearchNode::Regex(_) | SearchNode::Content(_) => false,
            _ => true,
        }
    }

    /// How many literal (non-wildcard) characters this subtree's text terms contribute: a sum across
    /// the whole query, so two single-character terms clear the floor together.
    pub fn literal_chars(&self) -> usize {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().map(SearchNode::literal_chars).sum(),
            // An exclusion never narrows, so its contents contribute nothing towards the floor.
            SearchNode::Not(_) => 0,
            SearchNode::Name { term, .. } | SearchNode::Path { term, .. } => {
                term.chars().filter(|c| !matches!(c, '*' | '?')).count()
            }
            SearchNode::Content(needle) => needle.chars().count(),
            _ => 0,
        }
    }

    /// Whether this subtree carries a filter specific enough to run with no text at all. An
    /// extension, a real size bound, a date or a regular expression are; a kind, a hidden flag or an
    /// attribute are not: half the disk is not a search result.
    pub fn has_filter(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().any(SearchNode::has_filter),
            SearchNode::Extension(_) | SearchNode::Date { .. } | SearchNode::Regex(_) => true,
            // A bound that excludes nothing is not a filter: "at least zero bytes" is everything.
            SearchNode::Size { lo, hi } => lo.is_some_and(|lo| lo > 0) || hi.is_some(),
            _ => false,
        }
    }

    /// Whether this node reads a field the sizeless index build never filled.
    pub fn needs_metadata(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().any(SearchNode::needs_metadata),
            SearchNode::Not(c) => c.needs_metadata(),
            SearchNode::Size { .. } | SearchNode::Date { .. } => true,
            _ => false,
        }
    }

    /// Whether any node asks for hidden entries (`is:hidden`). Not through a NOT: `!is:hidden` asks
    /// for the default, not to widen the scan.
    pub fn wants_hidden(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().any(SearchNode::wants_hidden),
            SearchNode::Hidden => true,
            _ => false,
        }
    }

    /// Whether any node asks for archive contents (`in:archives`). A scope, not a predicate, and
    /// not through a NOT for the same reason as `wants_hidden`.
    pub fn wants_archives(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().any(SearchNode::wants_archives),
            SearchNode::InArchives => true,
            _ => false,
        }
    }

    /// Whether any node asks for file contents. This one **does** propagate through a NOT:
    /// establishing that a file does not contain something needs it read exactly as much.
    pub fn needs_content(&self) -> bool {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => c.iter().any(SearchNode::needs_content),
            SearchNode::Not(c) => c.needs_content(),
            SearchNode::Content(_) => true,
            _ => false,
        }
    }

    /// Adds every `content:` needle in this subtree, in tree order.
    pub fn collect_content_needles(&self, into: &mut Vec<String>) {
        match self {
            SearchNode::And(c) | SearchNode::Or(c) => {
                c.iter().for_each(|n| n.collect_content_needles(into))
            }
            SearchNode::Not(c) => c.collect_content_needles(into),
            SearchNode::Content(needle) => into.push(needle.clone()),
            _ => {}
        }
    }
}
