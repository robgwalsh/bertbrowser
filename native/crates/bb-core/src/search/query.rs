//! A parsed search query: a tree of terms, ANDed by adjacency, with `OR`, `!`, brackets, quoted
//! phrases and the filter keys. Bare words keep the meaning they have always had: a case-insensitive
//! substring of the name with `*` and `?` honoured. Spec: Core/Services/Search/SearchQuery.cs.

use super::{
    SearchCandidate, SearchGrammar, SearchMatch, SearchNode, SearchQueryParse, SqlPredicate,
    SqlPredicateBuilder,
};

pub struct SearchQuery {
    root: SearchNode,
    text: String,
}

impl SearchQuery {
    pub(crate) fn new(root: SearchNode, text: &str) -> SearchQuery {
        SearchQuery {
            root,
            text: text.to_string(),
        }
    }

    /// The text this was parsed from, as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn root(&self) -> &SearchNode {
        &self.root
    }

    /// Parses query text. Never fails outward.
    pub fn parse(text: &str) -> SearchQueryParse {
        SearchGrammar::parse(text, true)
    }

    /// Whether `candidate` satisfies the query. The definition of a hit. **Undecided counts as a
    /// match here**: the repository's row re-check, the live scan and the archive scanner all ask
    /// "could this be a hit?", and a candidate whose file has not been read yet could be.
    pub fn matches(&self, candidate: &SearchCandidate<'_>) -> bool {
        self.root.matches(candidate) != SearchMatch::No
    }

    /// The full three-valued verdict, for the content pass.
    pub fn evaluate(&self, candidate: &SearchCandidate<'_>) -> SearchMatch {
        self.root.matches(candidate)
    }

    /// The WHERE fragment for an index query. Matches at least everything `matches` does; when
    /// `complete` is false the caller must re-check every row and must not push LIMIT down.
    pub fn compile(&self) -> SqlPredicate {
        let mut builder = SqlPredicateBuilder::new();
        self.root.write_sql(&mut builder);
        builder.build()
    }

    /// True when the query asks for hidden entries, which search otherwise excludes outright.
    pub fn wants_hidden(&self) -> bool {
        self.root.wants_hidden()
    }

    /// Whether the query asked for archive contents to be scanned as well.
    pub fn wants_archives(&self) -> bool {
        self.root.wants_archives()
    }

    /// True when the query filters on size or a date: columns empty on a volume the indexer built
    /// through its names-only fallback, so an empty result may be unanswerable rather than unmatched.
    pub fn needs_metadata(&self) -> bool {
        self.root.needs_metadata()
    }

    /// Whether the query asks for file contents, and so needs a second, reading pass.
    pub fn needs_content(&self) -> bool {
        self.root.needs_content()
    }

    /// Every `content:` needle in the query, in tree order. What the scanner highlights.
    pub fn content_needles(&self) -> Vec<String> {
        let mut needles = Vec::new();
        self.root.collect_content_needles(&mut needles);
        needles
    }
}
