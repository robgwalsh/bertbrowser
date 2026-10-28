//! A deliberately small, hand-rolled tokenizer: enough to make code legible in a preview pane,
//! and nothing more. It produces a flat, ordered, gap-free cover of the text (spans, not a tree)
//! so it has no parser, no error recovery and no opinion about whether the file is valid.
//!
//! Hand-rolled rather than taken from a package because the whole of it is rules, and rules
//! belong where a test can reach them. The failure mode that matters is not "the wrong colour":
//! it is a span that overlaps its neighbour or runs past the end of the string, which would throw
//! when the view builds runs from it. `syntax_tokenizer_tests` asserts the cover property over
//! every language on adversarial input, and `merge` degrades to plain text rather than returning
//! a cover that does not hold.
//!
//! Spans count **characters** (Unicode scalar values), not bytes: that is the unit the text view
//! lays out, and a byte offset into the middle of a multi-byte character is not a place a run can
//! start. Spec: Core/Services/Preview/SyntaxTokenizer.cs.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::classifier::extension_of;

/// What one run of characters is, for colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxClass {
    Text,
    Keyword,
    String,
    Comment,
    Number,
    Punctuation,
}

/// A run of `length` characters at `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyntaxSpan {
    pub start: usize,
    pub length: usize,
    pub class: SyntaxClass,
}

impl SyntaxSpan {
    pub fn new(start: usize, length: usize, class: SyntaxClass) -> SyntaxSpan {
        SyntaxSpan {
            start,
            length,
            class,
        }
    }
}

/// The syntax tables this app knows. `None` is not a failure: it is the answer for plain text,
/// and for any language we have no table for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxLanguage {
    None,
    CSharp,
    CFamily,
    JavaScript,
    Python,
    PowerShell,
    Shell,
    Sql,
    Css,
    Json,
    Xml,
    Ini,
    Markdown,
}

impl SyntaxLanguage {
    /// Every language, for a test that holds a property over all of them.
    pub const ALL: [SyntaxLanguage; 13] = [
        SyntaxLanguage::None,
        SyntaxLanguage::CSharp,
        SyntaxLanguage::CFamily,
        SyntaxLanguage::JavaScript,
        SyntaxLanguage::Python,
        SyntaxLanguage::PowerShell,
        SyntaxLanguage::Shell,
        SyntaxLanguage::Sql,
        SyntaxLanguage::Css,
        SyntaxLanguage::Json,
        SyntaxLanguage::Xml,
        SyntaxLanguage::Ini,
        SyntaxLanguage::Markdown,
    ];
}

pub struct SyntaxTokenizer;

impl SyntaxTokenizer {
    /// Which table applies to a file name. Extensionless well-known names are matched whole,
    /// because `Dockerfile` and `Makefile` have no extension to look at.
    pub fn language_for(name: &str) -> SyntaxLanguage {
        let extension = extension_of(name);
        if extension.is_empty() {
            return WHOLE_NAMES
                .get(bb_paths::file_name(name).to_lowercase().as_str())
                .copied()
                .unwrap_or(SyntaxLanguage::None);
        }

        let key = extension.to_lowercase();
        // ".gitignore" and friends: the extension of a leading-dot file is the whole name, which
        // is exactly the key WHOLE_NAMES holds.
        if let Some(dot_file) = WHOLE_NAMES.get(key.as_str()) {
            return *dot_file;
        }

        BY_EXTENSION
            .get(key.as_str())
            .copied()
            .unwrap_or(SyntaxLanguage::None)
    }

    /// Tokenizes `text`. The result covers every character exactly once, in order, with no gaps
    /// and no overlaps; an unknown language yields one `SyntaxClass::Text` span over the lot.
    pub fn tokenize(text: &str, language: SyntaxLanguage) -> Vec<SyntaxSpan> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return Vec::new();
        }
        if language == SyntaxLanguage::None {
            return vec![SyntaxSpan::new(0, chars.len(), SyntaxClass::Text)];
        }

        let spans = match language {
            SyntaxLanguage::Xml => scan_xml(&chars),
            SyntaxLanguage::Markdown => scan_markdown(&chars),
            _ => scan_generic(&chars, &RULES[&language]),
        };
        merge(spans, chars.len())
    }
}

// --- the generic scanner ---

struct LanguageRules {
    line_comments: &'static [&'static str],
    block_comments: &'static [(&'static str, &'static str)],
    string_delimiters: &'static [char],
    backslash_escapes: bool,
    keywords: HashSet<&'static str>,
}

fn scan_generic(text: &[char], rules: &LanguageRules) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let start = i;

        if try_block_comment(text, &mut i, rules) {
            spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::Comment));
            continue;
        }
        if try_line_comment(text, &mut i, rules) {
            spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::Comment));
            continue;
        }
        if try_string(text, &mut i, rules) {
            spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::String));
            continue;
        }

        let c = text[i];
        if c.is_ascii_digit() {
            i += 1;
            // Letters are consumed too, so 0xFF, 1e9 and 100L each stay one number rather than
            // becoming a number followed by an identifier that might collide with a keyword.
            while i < text.len() && (text[i].is_alphanumeric() || matches!(text[i], '.' | '_')) {
                i += 1;
            }
            spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::Number));
            continue;
        }

        if is_identifier_start(c) {
            i += 1;
            while i < text.len() && is_identifier_part(text[i]) {
                i += 1;
            }
            let word: String = text[start..i].iter().collect();
            let class = if rules.keywords.contains(word.as_str()) {
                SyntaxClass::Keyword
            } else {
                SyntaxClass::Text
            };
            spans.push(SyntaxSpan::new(start, i - start, class));
            continue;
        }

        i += 1;
        let class = if is_punctuation(c) {
            SyntaxClass::Punctuation
        } else {
            SyntaxClass::Text
        };
        spans.push(SyntaxSpan::new(start, 1, class));
    }
    spans
}

fn try_block_comment(text: &[char], i: &mut usize, rules: &LanguageRules) -> bool {
    for (open, close) in rules.block_comments {
        if !matches(text, *i, open) {
            continue;
        }
        // An unterminated block comment runs to the end of the file, which is what the compiler
        // would say too.
        *i = match index_of(text, close, *i + open.chars().count()) {
            Some(end) => end + close.chars().count(),
            None => text.len(),
        };
        return true;
    }
    false
}

fn try_line_comment(text: &[char], i: &mut usize, rules: &LanguageRules) -> bool {
    for token in rules.line_comments {
        if !matches(text, *i, token) {
            continue;
        }
        while *i < text.len() && !matches!(text[*i], '\n' | '\r') {
            *i += 1;
        }
        return true;
    }
    false
}

fn try_string(text: &[char], i: &mut usize, rules: &LanguageRules) -> bool {
    let quote = text[*i];
    if !rules.string_delimiters.contains(&quote) {
        return false;
    }

    *i += 1;
    while *i < text.len() {
        if rules.backslash_escapes && text[*i] == '\\' && *i + 1 < text.len() {
            *i += 2;
            continue;
        }
        if text[*i] == quote {
            *i += 1;
            return true;
        }
        // None of these languages lets an ordinary string span lines, and allowing it would let
        // one apostrophe swallow the rest of the file as a string.
        if text[*i] == '\n' {
            return true;
        }
        *i += 1;
    }
    true // unterminated: to end of file
}

// --- XML/HTML, which is shaped nothing like the others ---

fn scan_xml(text: &[char]) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let start = i;
        if matches(text, i, "<!--") {
            i = match index_of(text, "-->", i + 4) {
                Some(end) => end + 3,
                None => text.len(),
            };
            spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::Comment));
            continue;
        }

        if text[i] == '<' {
            i += 1;
            spans.push(SyntaxSpan::new(start, 1, SyntaxClass::Punctuation));

            // The element name, including a leading '/', '?' or '!'.
            let name_start = i;
            while i < text.len()
                && (is_identifier_part(text[i]) || matches!(text[i], '/' | '?' | '!' | ':'))
            {
                i += 1;
            }
            if i > name_start {
                spans.push(SyntaxSpan::new(
                    name_start,
                    i - name_start,
                    SyntaxClass::Keyword,
                ));
            }

            // Attributes, until the tag closes.
            while i < text.len() && text[i] != '>' {
                let attribute_start = i;
                if matches!(text[i], '"' | '\'') {
                    let quote = text[i];
                    i += 1;
                    while i < text.len() && text[i] != quote {
                        i += 1;
                    }
                    if i < text.len() {
                        i += 1;
                    }
                    spans.push(SyntaxSpan::new(
                        attribute_start,
                        i - attribute_start,
                        SyntaxClass::String,
                    ));
                    continue;
                }
                if is_identifier_start(text[i]) {
                    while i < text.len() && (is_identifier_part(text[i]) || text[i] == ':') {
                        i += 1;
                    }
                    spans.push(SyntaxSpan::new(
                        attribute_start,
                        i - attribute_start,
                        SyntaxClass::Text,
                    ));
                    continue;
                }
                i += 1;
                let class = if is_punctuation(text[attribute_start]) {
                    SyntaxClass::Punctuation
                } else {
                    SyntaxClass::Text
                };
                spans.push(SyntaxSpan::new(attribute_start, 1, class));
            }
            if i < text.len() {
                spans.push(SyntaxSpan::new(i, 1, SyntaxClass::Punctuation));
                i += 1;
            }
            continue;
        }

        // Body text, up to the next tag.
        while i < text.len() && text[i] != '<' {
            i += 1;
        }
        spans.push(SyntaxSpan::new(start, i - start, SyntaxClass::Text));
    }
    spans
}

// --- Markdown: a line-shaped language, so scanned by line ---

fn scan_markdown(text: &[char]) -> Vec<SyntaxSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    let mut in_fence = false;
    while i < text.len() {
        let line_start = i;
        while i < text.len() && !matches!(text[i], '\n' | '\r') {
            i += 1;
        }
        let line_end = i;
        while i < text.len() && matches!(text[i], '\n' | '\r') {
            i += 1;
        }

        let line = &text[line_start..line_end];
        let indent = line.iter().take_while(|c| c.is_whitespace()).count();
        let trimmed = &line[indent..];

        if starts_with(trimmed, "```") || starts_with(trimmed, "~~~") {
            in_fence = !in_fence;
            spans.push(SyntaxSpan::new(
                line_start,
                i - line_start,
                SyntaxClass::Punctuation,
            ));
            continue;
        }
        if in_fence {
            spans.push(SyntaxSpan::new(
                line_start,
                i - line_start,
                SyntaxClass::String,
            ));
            continue;
        }

        if trimmed.first() == Some(&'#') {
            spans.push(SyntaxSpan::new(
                line_start,
                i - line_start,
                SyntaxClass::Keyword,
            ));
            continue;
        }
        if trimmed.first() == Some(&'>') {
            spans.push(SyntaxSpan::new(
                line_start,
                i - line_start,
                SyntaxClass::Comment,
            ));
            continue;
        }

        // A bullet or a numbered marker: colour the marker, leave the text of the item alone.
        let marker = markdown_marker_length(trimmed);
        if marker > 0 {
            if indent > 0 {
                spans.push(SyntaxSpan::new(line_start, indent, SyntaxClass::Text));
            }
            spans.push(SyntaxSpan::new(
                line_start + indent,
                marker,
                SyntaxClass::Punctuation,
            ));
            spans.push(SyntaxSpan::new(
                line_start + indent + marker,
                i - line_start - indent - marker,
                SyntaxClass::Text,
            ));
            continue;
        }

        spans.push(SyntaxSpan::new(
            line_start,
            i - line_start,
            SyntaxClass::Text,
        ));
    }
    spans
}

fn markdown_marker_length(trimmed: &[char]) -> usize {
    if trimmed.len() >= 2 && matches!(trimmed[0], '-' | '*' | '+') && trimmed[1] == ' ' {
        return 2;
    }
    let digits = trimmed.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0
        && digits + 1 < trimmed.len()
        && matches!(trimmed[digits], '.' | ')')
        && trimmed[digits + 1] == ' '
    {
        return digits + 2;
    }
    0
}

// --- shared ---

fn matches(text: &[char], i: usize, token: &str) -> bool {
    (i..)
        .zip(token.chars())
        .all(|(at, c)| text.get(at) == Some(&c))
}

fn starts_with(text: &[char], token: &str) -> bool {
    matches(text, 0, token)
}

fn index_of(text: &[char], token: &str, from: usize) -> Option<usize> {
    (from..text.len()).find(|&at| matches(text, at, token))
}

fn is_identifier_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '$' | '@')
}

fn is_identifier_part(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '-')
}

fn is_punctuation(c: char) -> bool {
    c > ' ' && c < '\u{7F}' && !c.is_alphanumeric() && c != '_'
}

/// Collapses adjacent spans of the same class and drops empties, then checks the result really
/// does cover the text, the invariant the view depends on. A scanner bug becomes plain text here
/// rather than an exception three layers up.
fn merge(spans: Vec<SyntaxSpan>, length: usize) -> Vec<SyntaxSpan> {
    let mut merged: Vec<SyntaxSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.length == 0 {
            continue;
        }
        if let Some(last) = merged.last_mut()
            && last.class == span.class
            && last.start + last.length == span.start
        {
            last.length += span.length;
            continue;
        }
        merged.push(span);
    }

    let plain = || vec![SyntaxSpan::new(0, length, SyntaxClass::Text)];
    let mut covered = 0;
    for span in &merged {
        if span.start != covered || span.start + span.length > length {
            return plain();
        }
        covered += span.length;
    }
    if covered == length { merged } else { plain() }
}

// --- tables ---

fn words(words: &'static str) -> HashSet<&'static str> {
    words.split_ascii_whitespace().collect()
}

static RULES: LazyLock<HashMap<SyntaxLanguage, LanguageRules>> = LazyLock::new(|| {
    HashMap::from([
        (
            SyntaxLanguage::CSharp,
            LanguageRules {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['"', '\''],
                backslash_escapes: true,
                keywords: words(
                    "abstract as async await base bool break byte case catch char checked class const continue decimal default delegate do double dynamic else enum event explicit extern false finally fixed float for foreach get global goto if implicit in init int interface internal is lock long namespace new nint nuint null object operator out override params partial private protected public readonly record ref return sbyte sealed set short sizeof stackalloc static string struct switch this throw true try typeof uint ulong unchecked unsafe ushort using var virtual void volatile when where while with yield",
                ),
            },
        ),
        (
            SyntaxLanguage::CFamily,
            LanguageRules {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['"', '\''],
                backslash_escapes: true,
                keywords: words(
                    "auto bool break case catch char class const constexpr continue default delete do double else enum explicit extern false float for fn friend func goto if impl inline int interface let long mut namespace new nil nullptr operator package private protected pub public range register return self short signed sizeof static struct switch template this throw true try type typedef typename union unsigned use using val var virtual void volatile where while",
                ),
            },
        ),
        (
            SyntaxLanguage::JavaScript,
            LanguageRules {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['"', '\'', '`'],
                backslash_escapes: true,
                keywords: words(
                    "abstract any as async await boolean break case catch class const constructor continue debugger declare default delete do else enum export extends false finally for from function get if implements import in instanceof interface keyof let namespace never new null number of private protected public readonly return set static string super switch symbol this throw true try type typeof undefined unknown var void while with yield",
                ),
            },
        ),
        (
            SyntaxLanguage::Json,
            LanguageRules {
                // JSON proper has no comments, but a .json file in a source tree very often does
                // (tsconfig, launch.json, devcontainer). Colouring them is the useful behaviour.
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['"'],
                backslash_escapes: true,
                keywords: words("true false null"),
            },
        ),
        (
            SyntaxLanguage::Python,
            LanguageRules {
                line_comments: &["#"],
                block_comments: &[("\"\"\"", "\"\"\""), ("'''", "'''")],
                string_delimiters: &['"', '\''],
                backslash_escapes: true,
                keywords: words(
                    "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return self True try while with yield",
                ),
            },
        ),
        (
            SyntaxLanguage::PowerShell,
            LanguageRules {
                line_comments: &["#"],
                block_comments: &[("<#", "#>")],
                string_delimiters: &['"', '\''],
                backslash_escapes: false,
                keywords: words(
                    "begin break catch class continue data define do dynamicparam else elseif end enum exit filter finally for foreach from function hidden if in param process return static switch throw trap try until using while workflow",
                ),
            },
        ),
        (
            SyntaxLanguage::Shell,
            LanguageRules {
                line_comments: &["#"],
                block_comments: &[],
                string_delimiters: &['"', '\''],
                backslash_escapes: false,
                keywords: words(
                    "if then else elif fi case esac for while until do done in function return break continue local export readonly declare source eval exec set unset shift trap exit echo",
                ),
            },
        ),
        (
            SyntaxLanguage::Sql,
            LanguageRules {
                line_comments: &["--"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['\'', '"'],
                backslash_escapes: false,
                keywords: words(
                    "ADD ALL ALTER AND AS ASC BEGIN BETWEEN BY CASE CAST COLUMN COMMIT CONSTRAINT CREATE CROSS DECLARE DEFAULT DELETE DESC DISTINCT DROP ELSE END EXEC EXISTS FOREIGN FROM FULL GROUP HAVING IF IN INDEX INNER INSERT INTO IS JOIN KEY LEFT LIKE LIMIT NOT NULL OFFSET ON OR ORDER OUTER PRIMARY REFERENCES RIGHT ROLLBACK SELECT SET TABLE THEN TOP TRANSACTION UNION UNIQUE UPDATE VALUES VIEW WHEN WHERE WITH add all alter and as asc begin between by case cast column commit constraint create cross declare default delete desc distinct drop else end exec exists foreign from full group having if in index inner insert into is join key left like limit not null offset on or order outer primary references right rollback select set table then top transaction union unique update values view when where with",
                ),
            },
        ),
        (
            SyntaxLanguage::Css,
            LanguageRules {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &['"', '\''],
                backslash_escapes: true,
                keywords: words(
                    "important media import charset keyframes supports and not only from to",
                ),
            },
        ),
        (
            SyntaxLanguage::Ini,
            LanguageRules {
                line_comments: &["#", ";"],
                block_comments: &[],
                string_delimiters: &['"', '\''],
                backslash_escapes: false,
                keywords: words("true false yes no on off True False"),
            },
        ),
    ])
});

/// Keyed by lower-cased extension, dot included.
static BY_EXTENSION: LazyLock<HashMap<&'static str, SyntaxLanguage>> = LazyLock::new(|| {
    use SyntaxLanguage::*;
    let mut map = HashMap::new();
    let mut add = |extensions: &'static str, language: SyntaxLanguage| {
        for extension in extensions.split_ascii_whitespace() {
            map.insert(extension, language);
        }
    };
    add(".cs .csx", CSharp);
    add(
        ".c .h .cpp .hpp .cc .hh .cxx .mm .java .kt .kts .swift .go .rs .php .scala .dart .groovy .gradle .proto .zig .nim",
        CFamily,
    );
    add(".js .mjs .cjs .jsx .ts .tsx", JavaScript);
    add(
        ".json .jsonc .json5 .webmanifest .ipynb .prettierrc .eslintrc .babelrc .stylelintrc",
        Json,
    );
    add(
        ".xml .xaml .html .htm .xhtml .svg .csproj .fsproj .vbproj .props .targets .config .resx .nuspec .plist \
         .manifest .appxmanifest .xsd .xsl .xslt .dtd .rss .atom .opml .sitemap .axaml \
         .vcxproj .shproj .pubxml .ruleset .runsettings .wxs .wxi .storyboard \
         .cshtml .razor .vbhtml .aspx .ascx .master .jsp",
        Xml,
    );
    add(
        ".gitmodules .gitconfig .npmignore .htaccess .ini .cfg .conf .toml .yml .yaml .properties",
        Ini,
    );
    add(".css .scss .less", Css);
    add(".sql", Sql);
    add(".py .pyw .pyi", Python);
    add(".ps1 .psm1 .psd1", PowerShell);
    add(".sh .bash .zsh .bat .cmd", Shell);
    add(".md .markdown .mdx", Markdown);
    map
});

/// Keyed by lower-cased whole file name.
static WHOLE_NAMES: LazyLock<HashMap<&'static str, SyntaxLanguage>> = LazyLock::new(|| {
    HashMap::from([
        ("dockerfile", SyntaxLanguage::Shell),
        ("makefile", SyntaxLanguage::Shell),
        ("rakefile", SyntaxLanguage::Shell),
        (".gitignore", SyntaxLanguage::Ini),
        (".gitattributes", SyntaxLanguage::Ini),
        (".dockerignore", SyntaxLanguage::Ini),
        (".editorconfig", SyntaxLanguage::Ini),
        (".npmrc", SyntaxLanguage::Ini),
        (".env", SyntaxLanguage::Ini),
    ])
});
