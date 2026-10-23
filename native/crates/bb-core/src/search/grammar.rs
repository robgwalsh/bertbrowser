//! Turns query text into a `SearchNode` tree. **Never fails outward**: every problem (a regular
//! expression that will not compile, an unparseable size, a half-typed `size:>`) comes back as text
//! for the user; this runs on a keystroke. **Total, and biased towards a wider result**: a trailing
//! `!`, an unbalanced `)`, an unclosed `(` and an unrecognised `key:` are all ordinary text. A search
//! box that says "no" to something a filename could contain is worse than one that searches for it.
//! Spec: Core/Services/Search/SearchGrammar.cs.

use bb_paths::fold_upper;

use super::{DateField, DateShorthand, SearchNode, SearchQuery, SearchSyntax, SizeText};
use crate::FileTime;
use crate::attributes;

/// The outcome of parsing a query box. `query` is None when there is nothing to run (an empty box,
/// or text too broad to be a useful search); with `problem` also None that means "not a search" and
/// the caller shows the plain listing. A problem is different: the caller stays in search mode and
/// shows the message rather than flipping back to the folder mid-keystroke.
pub struct SearchQueryParse {
    pub query: Option<SearchQuery>,
    pub problem: Option<String>,
}

pub struct SearchGrammar;

/// The backtracking budget a user-supplied regular expression gets per name. `(a+)+$` is three
/// keystrokes away in any search box; patterns without lookaround or backreferences run in linear
/// time regardless, and the others stop here.
const REGEX_BACKTRACK_LIMIT: usize = 200_000;

impl SearchGrammar {
    /// `require_narrow`: whether the query has to be narrow enough to run over a disk. False for a
    /// caller that judges a listing already on screen (select by pattern), where `*` or
    /// `is:readonly` on its own is an ordinary thing to ask.
    pub fn parse(text: &str, require_narrow: bool) -> SearchQueryParse {
        Self::parse_at(text, require_narrow, FileTime::now().to_local())
    }

    /// As [`parse`](Self::parse), with the local clock the date shorthands resolve against.
    pub fn parse_at(text: &str, require_narrow: bool, now_local: FileTime) -> SearchQueryParse {
        if text.trim().is_empty() {
            return SearchQueryParse {
                query: None,
                problem: None,
            };
        }
        let tokens = lex(text);
        if tokens.is_empty() {
            return SearchQueryParse {
                query: None,
                problem: None,
            };
        }
        let mut parser = Parser {
            tokens,
            at: 0,
            problem: None,
            now_local,
        };
        let root = parser.parse_or();
        if let Some(problem) = parser.problem {
            return SearchQueryParse {
                query: None,
                problem: Some(problem),
            };
        }
        let Some(root) = root else {
            return SearchQueryParse {
                query: None,
                problem: None,
            };
        };

        // Refused rather than answered wrongly: there is no file on disk behind an archive entry,
        // and an unread candidate counts as a possible match, so an archive scan would report every
        // entry in every container as a hit.
        if root.needs_content() && root.wants_archives() {
            return SearchQueryParse {
                query: None,
                problem: Some(
                    "content: can't look inside archives \u{2014} extract them first.".to_string(),
                ),
            };
        }

        // The floor: something has to narrow the disk down. Two literal characters of text, summed
        // across the query, or a filter specific enough to stand alone.
        if require_narrow && root.literal_chars() < 2 && !root.has_filter() {
            return SearchQueryParse {
                query: None,
                problem: None,
            };
        }
        SearchQueryParse {
            query: Some(SearchQuery::new(root, text)),
            problem: None,
        }
    }
}

// ---------------- Lexing ----------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Term,
    Or,
    Not,
    Open,
    Close,
}

struct Token {
    kind: TokenKind,
    text: String,
    /// A quote appeared somewhere in the token, so its text is literal.
    quoted: bool,
    /// Byte index of the first unquoted colon, or None.
    colon: Option<usize>,
    /// A quote appeared *before* any colon, so what precedes that colon is not a key.
    quoted_key: bool,
}

fn op(kind: TokenKind, text: &str) -> Token {
    Token {
        kind,
        text: text.to_string(),
        quoted: false,
        colon: None,
        quoted_key: false,
    }
}

/// Splits the text into terms and operators. Quotes suppress every special meaning inside them.
fn lex(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '(' {
            tokens.push(op(TokenKind::Open, "("));
            i += 1;
            continue;
        }
        if c == ')' {
            tokens.push(op(TokenKind::Close, ")"));
            i += 1;
            continue;
        }
        // A '!' only negates something. Trailing, or followed by a space, it is a character in a
        // filename and nothing more.
        if c == '!' && i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
            tokens.push(op(TokenKind::Not, "!"));
            i += 1;
            continue;
        }

        let (token, next) = lex_term(&chars, i);
        i = next;

        // Operators are uppercase only: lowercase "or" stays a word, so a file named
        // "Report or Draft" keeps being findable.
        if !token.quoted && token.colon.is_none() {
            match token.text.as_str() {
                "OR" | "|" => {
                    tokens.push(op(TokenKind::Or, "OR"));
                    continue;
                }
                "NOT" => {
                    tokens.push(op(TokenKind::Not, "!"));
                    continue;
                }
                "AND" => continue, // adjacency already means AND
                _ => {}
            }
        }
        tokens.push(token);
    }
    tokens
}

/// Reads one term, honouring quoted runs, and notes where its first unquoted colon was.
fn lex_term(chars: &[char], start: usize) -> (Token, usize) {
    let mut text = String::new();
    let mut quoted = false;
    let mut quoted_key = false;
    let mut colon: Option<usize> = None;
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            break;
        }
        // Brackets group, except inside a regular expression, where they are structural:
        // `re:(a+)+$` has to survive as one token.
        if (c == '(' || c == ')') && !is_regex_value(&text, colon) {
            break;
        }
        if c == '"' {
            quoted = true;
            // A quote before the colon means there is no key; one after it is quoting the value.
            if colon.is_none() {
                quoted_key = true;
            }
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                text.push(chars[i]);
                i += 1;
            }
            // An unclosed quote runs to the end rather than being an error.
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        if c == ':' && colon.is_none() {
            colon = Some(text.len());
        }
        text.push(c);
        i += 1;
    }
    (
        Token {
            kind: TokenKind::Term,
            text,
            quoted,
            colon,
            quoted_key,
        },
        i,
    )
}

/// Whether what has been read so far is the `re:` key and its value.
fn is_regex_value(text: &str, colon: Option<usize>) -> bool {
    match colon {
        Some(c) if c > 0 => {
            SearchSyntax::resolve(&fold_upper(&text[..c])) == Some(SearchSyntax::REGEX)
        }
        _ => false,
    }
}

// ---------------- Parsing ----------------

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    /// The first thing that made the query unusable, if anything did.
    problem: Option<String>,
    now_local: FileTime,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    /// OR binds loosest, so it is the outermost level.
    fn parse_or(&mut self) -> Option<SearchNode> {
        let mut branches = Vec::new();
        if let Some(first) = self.parse_and() {
            branches.push(first);
        }
        while self.peek().is_some_and(|t| t.kind == TokenKind::Or) {
            self.at += 1;
            if let Some(next) = self.parse_and() {
                branches.push(next);
            }
        }
        match branches.len() {
            0 => None,
            1 => branches.pop(),
            _ => Some(SearchNode::Or(branches)),
        }
    }

    /// Adjacency is AND.
    fn parse_and(&mut self) -> Option<SearchNode> {
        let mut parts = Vec::new();
        while self
            .peek()
            .is_some_and(|t| !matches!(t.kind, TokenKind::Or | TokenKind::Close))
        {
            if let Some(part) = self.parse_unary() {
                parts.push(part);
            }
            if self.problem.is_some() {
                return None;
            }
        }
        match parts.len() {
            0 => None,
            1 => parts.pop(),
            _ => Some(SearchNode::And(parts)),
        }
    }

    fn parse_unary(&mut self) -> Option<SearchNode> {
        if self.peek().is_some_and(|t| t.kind == TokenKind::Not) {
            self.at += 1;
            return self
                .parse_unary()
                .map(|inner| SearchNode::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<SearchNode> {
        let token = self.peek()?;
        match token.kind {
            TokenKind::Open => {
                self.at += 1;
                let inner = self.parse_or();
                // An unclosed '(' closes at the end of the text rather than failing.
                if self.peek().is_some_and(|t| t.kind == TokenKind::Close) {
                    self.at += 1;
                }
                inner
            }
            // Unbalanced ')': ordinary text, and a character filenames genuinely contain.
            TokenKind::Close => {
                self.at += 1;
                Some(SearchNode::name(")", true))
            }
            _ => {
                self.at += 1;
                let token = &self.tokens[self.at - 1];
                let (text, quoted, colon, quoted_key) = (
                    token.text.clone(),
                    token.quoted,
                    token.colon,
                    token.quoted_key,
                );
                self.build_term(&text, quoted, colon, quoted_key)
            }
        }
    }

    /// Turns one term token into a node, resolving its key if it has one. A key is still a key when
    /// its *value* is quoted (`content:"a phrase"`); a quote before the colon means the whole token
    /// is literal text that happens to contain a colon, so `"C:\Users"` stays a name.
    fn build_term(
        &mut self,
        text: &str,
        quoted: bool,
        colon: Option<usize>,
        quoted_key: bool,
    ) -> Option<SearchNode> {
        if let Some(c) = colon
            && !quoted_key
            && c > 0
        {
            let key = fold_upper(&text[..c]);
            let value = &text[c + 1..];
            if let Some(reason) = SearchSyntax::unsupported_reason(&key) {
                self.problem = Some(format!("{}: {reason}.", key.to_lowercase()));
                return None;
            }
            if let Some(canonical) = SearchSyntax::resolve(&key) {
                return self.build_filter(canonical, &key, value, quoted);
            }
        }
        if text.is_empty() {
            return None;
        }
        Some(SearchNode::name(&fold_upper(text), quoted))
    }

    fn build_filter(
        &mut self,
        key: &str,
        typed_key: &str,
        value: &str,
        quoted: bool,
    ) -> Option<SearchNode> {
        if value.is_empty() {
            self.problem = Some(format!("{}: needs a value.", typed_key.to_lowercase()));
            return None;
        }
        match key {
            SearchSyntax::NAME => Some(SearchNode::name(&fold_upper(value), quoted)),
            SearchSyntax::PATH => Some(SearchNode::path(&fold_upper(value))),
            // Deliberately not folded: the other side is file text folded per file otherwise.
            SearchSyntax::CONTENT => {
                if value.chars().count() < 2 {
                    return self.fail("content: needs at least two characters to look for.");
                }
                Some(SearchNode::Content(value.to_string()))
            }
            SearchSyntax::EXTENSION => {
                let extensions: Vec<String> = value
                    .split([';', ','])
                    .map(|e| fold_upper(e.trim_start_matches('.')))
                    .filter(|e| !e.is_empty())
                    .collect();
                if extensions.is_empty() {
                    return self.fail("ext: needs at least one extension, like ext:jpg.");
                }
                Some(SearchNode::Extension(extensions))
            }
            SearchSyntax::SIZE => self.build_size(value),
            SearchSyntax::MODIFIED => self.build_date(DateField::Modified, "dm", value),
            SearchSyntax::CREATED => self.build_date(DateField::Created, "dc", value),
            SearchSyntax::IS => match fold_upper(value).as_str() {
                "DIR" | "DIRECTORY" | "FOLDER" => Some(SearchNode::Kind { is_directory: true }),
                "FILE" => Some(SearchNode::Kind { is_directory: false }),
                "HIDDEN" => Some(SearchNode::Hidden),
                "READONLY" | "READ-ONLY" | "RO" => Some(SearchNode::Attribute(attributes::READONLY)),
                "SYSTEM" => Some(SearchNode::Attribute(attributes::SYSTEM)),
                // "archived", not "archive", because in:archives already means "look inside zips";
                // the competitors' spelling is accepted too.
                "ARCHIVED" | "ARCHIVE" => Some(SearchNode::Attribute(attributes::ARCHIVE)),
                "LINK" | "JUNCTION" | "SYMLINK" | "REPARSE" => Some(SearchNode::Attribute(attributes::REPARSE_POINT)),
                "COMPRESSED" => Some(SearchNode::Attribute(attributes::COMPRESSED)),
                _ => self.fail(&format!(
                    "is: doesn't know '{value}' \u{2014} try is:dir, is:file, is:hidden, is:readonly, is:system, is:archived, is:link or is:compressed."
                )),
            },
            SearchSyntax::IN => match fold_upper(value).as_str() {
                "ARCHIVE" | "ARCHIVES" | "ZIP" | "ZIPS" => Some(SearchNode::InArchives),
                _ => self.fail(&format!("in: doesn't know '{value}' \u{2014} the only scope is in:archives.")),
            },
            SearchSyntax::REGEX => match fancy_regex::RegexBuilder::new(&format!("(?i){value}"))
                .backtrack_limit(REGEX_BACKTRACK_LIMIT)
                .build()
            {
                Ok(regex) => Some(SearchNode::Regex(regex)),
                Err(e) => self.fail(&format!("That is not a valid regular expression: {e}")),
            },
            _ => None,
        }
    }

    fn build_size(&mut self, value: &str) -> Option<SearchNode> {
        let (op, rest) = split_operator(value);
        // Mid-type: the operator is there and the number is not yet.
        if rest.is_empty() {
            return self.fail("size: needs a number, like size:>100mb.");
        }
        if let Some((a, b)) = rest.split_once("..") {
            let (Some(mut from), Some(mut to)) = (SizeText::try_parse(a), SizeText::try_parse(b))
            else {
                return self.fail(&format!(
                    "size: can't read the range '{rest}' \u{2014} try size:1mb..2mb."
                ));
            };
            if to < from {
                std::mem::swap(&mut from, &mut to);
            }
            return Some(SearchNode::Size {
                lo: Some(from),
                hi: bump(to),
            });
        }
        if rest.eq_ignore_ascii_case("empty") {
            return Some(SearchNode::Size {
                lo: None,
                hi: Some(1),
            });
        }
        let Some(bytes) = SizeText::try_parse(rest) else {
            return self.fail(&format!(
                "size: can't read '{rest}' \u{2014} try size:>100mb."
            ));
        };
        Some(match op {
            ">" => SearchNode::Size {
                lo: bump(bytes),
                hi: None,
            },
            ">=" => SearchNode::Size {
                lo: Some(bytes),
                hi: None,
            },
            "<" => SearchNode::Size {
                lo: None,
                hi: Some(bytes),
            },
            "<=" => SearchNode::Size {
                lo: None,
                hi: bump(bytes),
            },
            _ => SearchNode::Size {
                lo: Some(bytes),
                hi: bump(bytes),
            },
        })
    }

    /// `key` is the key as the user spelled it, so `dc:` is not told what `dm:` accepts.
    fn build_date(&mut self, field: DateField, key: &str, value: &str) -> Option<SearchNode> {
        let (op, rest) = split_operator(value);
        let now = self.now_local;
        if rest.is_empty() {
            return self.fail(&format!(
                "{key}: needs a date, like {key}:today or {key}:>2026-01-01."
            ));
        }
        if let Some((a, b)) = rest.split_once("..") {
            let (Some((from_lo, _)), Some((_, to_hi))) = (
                DateShorthand::try_resolve(a, now),
                DateShorthand::try_resolve(b, now),
            ) else {
                return self.fail(&format!(
                    "{key}: can't read the range '{rest}' \u{2014} try {key}:2026-01-01..2026-06-30."
                ));
            };
            return Some(SearchNode::Date {
                lo: Some(from_lo),
                hi: Some(to_hi),
                field,
            });
        }
        let Some((lo, hi)) = DateShorthand::try_resolve(rest, now) else {
            return self.fail(&format!(
                "{key}: can't read '{rest}' \u{2014} try {key}:today or {key}:2026-08."
            ));
        };
        Some(match op {
            ">" => SearchNode::Date {
                lo: Some(hi),
                hi: None,
                field,
            },
            ">=" => SearchNode::Date {
                lo: Some(lo),
                hi: None,
                field,
            },
            "<" => SearchNode::Date {
                lo: None,
                hi: Some(lo),
                field,
            },
            "<=" => SearchNode::Date {
                lo: None,
                hi: Some(hi),
                field,
            },
            _ => SearchNode::Date {
                lo: Some(lo),
                hi: Some(hi),
                field,
            },
        })
    }

    fn fail(&mut self, problem: &str) -> Option<SearchNode> {
        if self.problem.is_none() {
            self.problem = Some(problem.to_string());
        }
        None
    }
}

fn split_operator(value: &str) -> (&'static str, &str) {
    for op in [">=", "<=", ">", "<", "="] {
        if let Some(rest) = value.strip_prefix(op) {
            return (op, rest.trim());
        }
    }
    ("", value.trim())
}

/// The exclusive upper bound one past a value, without overflowing.
fn bump(value: i64) -> Option<i64> {
    if value == i64::MAX {
        None
    } else {
        Some(value + 1)
    }
}
