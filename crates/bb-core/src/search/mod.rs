//! The search query language: grammar, the term tree with its two faces (`matches` is the
//! definition, `write_sql` an optimisation that may be wider but never narrower), size and date
//! literals, content text and snippets. Spec: Core/Services/Search/*.

mod candidate;
mod content_reader;
mod content_scanner;
mod content_text;
mod date_shorthand;
mod glob;
mod grammar;
mod node;
mod predicate;
mod query;
mod rules;
mod size_text;
mod snippet;
mod syntax;

pub use candidate::SearchCandidate;
pub use content_reader::{ContentReader, FileSystemContentReader, FileText};
pub use content_scanner::{ContentProgressSink, ContentScanOutcome, ContentScanner, LiveBatches};
pub use content_text::{ContentMatch, ContentText};
pub use date_shorthand::DateShorthand;
pub use glob::GlobText;
pub use grammar::{SearchGrammar, SearchQueryParse};
pub use node::{DateField, SearchNode};
pub use predicate::{SqlPredicate, SqlPredicateBuilder};
pub use query::SearchQuery;
pub use rules::ContentSearchRules;
pub use size_text::SizeText;
pub use snippet::ContentSnippet;
pub use syntax::{SearchSyntax, SearchSyntaxEntry, SearchSyntaxSection};

/// What a node made of one candidate: it matched, it did not, or it cannot be answered until the
/// file has been read. Three-valued because the query is asked twice: a `content:` term is a
/// question about bytes, and the first pass has none. In `content:a OR ext:txt` every `.txt` is
/// already a hit and must never be read; in `is:dir content:x` every directory is already refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMatch {
    /// Settled: not a hit, whatever its contents turn out to be.
    No,
    /// Settled: a hit, without reading it.
    Yes,
    /// Undecided until the file is read. Only a content term originates this.
    NeedsContent,
}

impl SearchMatch {
    pub(crate) fn verdict(matched: bool) -> SearchMatch {
        if matched {
            SearchMatch::Yes
        } else {
            SearchMatch::No
        }
    }
}
