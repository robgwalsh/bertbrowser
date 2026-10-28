//! Turns what the user asked for into the name each selected item gets. The plain box is one rule —
//! one item takes the typed text as its whole name, several are numbered while keeping their own
//! extensions. The expanded panel adds find/replace, a case transform, a counter and a date,
//! described by `RenameRule`. Spec: Core/Services/Rename/RenamePattern.cs.
//!
//! Pure, and separate from `RenamePlanner`, because the dialog previews the result of every
//! keystroke: the naming rule and the "is this a legal Windows name" rule have to be the same ones
//! the rename itself will use, not a re-implementation that can drift.
//!
//! **Nothing here fails.** The planner calls it unguarded and the dialog calls that on the UI
//! thread, so a regular expression that will not compile, one that runs past its backtrack limit,
//! and a date format that is not one all come back as text — a `validate_rule` message, or a
//! per-item `RenamedName::problem`.

use bb_paths::file_name;
use fancy_regex::Regex;

use super::date_format::format_date;
use super::paths::{directory_name, split_extension, trim_ending_separator};
use super::regex_replace;
use super::{
    MetadataFieldKind, MetadataFields, RenameCase, RenamePart, RenameRule, RenameScope,
    RenameSegment, RenameSource, RenameTemplate,
};
use crate::FileTime;

/// The name the rule produced, and why this one item could not be named properly, or None. Kept
/// per item rather than failing the batch, because one item's missing date must not cost the other
/// 299 their preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenamedName {
    pub name: String,
    pub problem: Option<String>,
}

/// Names Windows still refuses to give a file, with or without an extension.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `Path.GetInvalidFileNameChars()` beyond the control characters, which `char::is_control` covers.
const INVALID_CHARS: [char; 9] = ['"', '<', '>', '|', ':', '*', '?', '\\', '/'];

pub struct RenamePattern;

impl RenamePattern {
    /// NTFS's per-component limit.
    pub const MAX_NAME_LENGTH: usize = 255;

    /// What `{modified}` means with no format of its own: sortable, and legal.
    pub const DEFAULT_DATE_FORMAT: &'static str = "yyyy-MM-dd";

    /// The names `pattern` gives `sources`, one per source and in the same order, through the
    /// plain box's literal rule. Never fails: an unusable pattern still produces names, and
    /// `validate` is what rejects them.
    pub fn apply_simple(sources: &[RenameSource], pattern: &str) -> Vec<String> {
        Self::apply(sources, &RenameRule::simple(pattern))
            .into_iter()
            .map(|r| r.name)
            .collect()
    }

    /// The names `rule` gives `sources`, one per source and in the same order, each carrying
    /// whatever went wrong for that item alone.
    pub fn apply(sources: &[RenameSource], rule: &RenameRule) -> Vec<RenamedName> {
        if sources.is_empty() {
            return Vec::new();
        }

        let text = clean(&rule.template);

        if rule.is_literal {
            return literally(sources, &text);
        }

        let segments = match RenameTemplate::parse(&text) {
            Ok(segments) => segments,
            Err(problem) => return everything(sources, Some(problem)),
        };

        let mut regex = None;
        if rule.use_regex && !rule.find.is_empty() {
            match regex_replace::build(&rule.find, rule.match_case) {
                Ok(r) => regex = Some(r),
                Err(e) => {
                    return everything(
                        sources,
                        Some(format!("That is not a valid regular expression: {e}")),
                    );
                }
            }
        }

        sources
            .iter()
            .enumerate()
            .map(|(i, source)| one(source, i, rule, &segments, regex.as_ref()))
            .collect()
    }

    /// Why `rule` cannot be used at all, or None when it can. Separate from `validate`, which
    /// judges one finished name and is shared with `NewItemPattern`: this one never touches name
    /// legality.
    pub fn validate_rule(rule: &RenameRule) -> Option<String> {
        if rule.is_literal {
            return None;
        }

        if rule.counter_step == 0 {
            return Some(
                "Counter step can't be zero \u{2014} every item would be given the same number."
                    .to_string(),
            );
        }

        let segments = match RenameTemplate::parse(&clean(&rule.template)) {
            Ok(segments) => segments,
            Err(problem) => return Some(problem),
        };

        if rule.use_regex
            && !rule.find.is_empty()
            && let Err(e) = regex_replace::build(&rule.find, rule.match_case)
        {
            return Some(format!("That is not a valid regular expression: {e}"));
        }

        for segment in &segments {
            // A date from the file's own metadata is formatted by the same rules as its modified
            // date.
            let format = match segment.part {
                RenamePart::Modified => segment.argument.as_str(),
                RenamePart::Tag => match RenameTemplate::tag_of(segment) {
                    (spec, argument) if spec.kind == MetadataFieldKind::Date => argument,
                    _ => "",
                },
                _ => "",
            };
            if format.is_empty() {
                continue;
            }

            let Ok(formatted) = format_date(sample_date(), format) else {
                return Some(format!(
                    "'{format}' is not a date format. Try {}.",
                    Self::DEFAULT_DATE_FORMAT
                ));
            };

            // A standard format such as "d" is perfectly valid and produces slashes; the refusal
            // that followed would name the character rather than the format that put it there.
            if formatted.chars().any(|c| INVALID_CHARS.contains(&c)) {
                return Some(format!(
                    "A date written '{format}' comes out as '{formatted}', which a name can't hold. Try {}.",
                    Self::DEFAULT_DATE_FORMAT
                ));
            }
        }

        None
    }

    /// What the dialog starts with: the one item's whole name, or — for a selection — the first
    /// item's name without its extension, which is the part a numbered rename replaces.
    pub fn suggest_for(sources: &[RenameSource]) -> String {
        let Some(first) = sources.first() else {
            return String::new();
        };
        let name = file_name(&first.path);
        if sources.len() == 1 || first.is_directory {
            return name.to_string();
        }
        split_extension(name).0.to_string()
    }

    /// The length, in UTF-16 units as a text box counts them, of the part of a single item's name
    /// that a rename usually replaces, so the dialog can pre-select it and leave the extension
    /// alone.
    pub fn base_name_length(source: &RenameSource) -> usize {
        Self::split(source).0.encode_utf16().count()
    }

    /// Why `name` can't be a file name, or None when it can.
    pub fn validate(name: &str) -> Option<String> {
        if name.trim().is_empty() {
            return Some("Enter a name.".to_string());
        }

        if name.encode_utf16().count() > Self::MAX_NAME_LENGTH {
            return Some(format!(
                "A name can be at most {} characters.",
                Self::MAX_NAME_LENGTH
            ));
        }

        for c in name.chars() {
            if c.is_control() {
                return Some("A name can't contain control characters.".to_string());
            }
            if INVALID_CHARS.contains(&c) {
                return Some("A name can't contain any of  \\ / : * ? \" < > |".to_string());
            }
        }

        if name.ends_with(['.', ' ']) {
            return Some("A name can't end with a space or a period.".to_string());
        }

        let stem = name.split('.').next().unwrap_or(name);
        if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Some(format!("'{stem}' is a name Windows reserves for a device."));
        }

        None
    }

    /// The one place a name is cut into the part before its extension and the extension itself.
    ///
    /// Two carve-outs, both of which already exist elsewhere and must not be re-decided here. A
    /// folder has no extension, so "My.Project" is not ".Project" over "My". And a dotfile such as
    /// .gitignore is all *extension* as far as `System.IO.Path` was concerned, which would leave a
    /// find/replace scoped to the name with nothing to work on while an extension scope rewrote
    /// the whole thing — so it is treated as all stem.
    pub fn split(source: &RenameSource) -> (String, String) {
        let (stem, extension) = split_name(file_name(&source.path), source.is_directory);
        (stem.to_string(), extension.to_string())
    }
}

/// Exactly what a rename has always done with a typed name: one item takes it whole, several are
/// numbered and keep their own extensions.
///
/// The text is cleaned *before* the number and extension go on, which is where the cleaning has
/// always happened and has to stay: doing it afterwards would leave "  Holiday  " over two files as
/// "Holiday   1.jpg". An empty pattern produces an empty name rather than " 1.jpg" — a name
/// `validate` would otherwise accept, from a box the user had merely cleared.
fn literally(sources: &[RenameSource], text: &str) -> Vec<RenamedName> {
    if sources.len() == 1 {
        return vec![RenamedName {
            name: text.to_string(),
            problem: None,
        }];
    }
    sources
        .iter()
        .enumerate()
        .map(|(i, source)| RenamedName {
            name: if text.is_empty() {
                String::new()
            } else {
                format!("{text} {}{}", i + 1, RenamePattern::split(source).1)
            },
            problem: None,
        })
        .collect()
}

fn one(
    source: &RenameSource,
    index: usize,
    rule: &RenameRule,
    segments: &[RenameSegment],
    regex: Option<&Regex>,
) -> RenamedName {
    let (mut stem, mut extension) = RenamePattern::split(source);
    let mut problem = None;

    match rewrite(&stem, &extension, source.is_directory, rule, regex) {
        Ok((s, e)) => {
            stem = s;
            extension = e;
        }
        Err(regex_replace::MatchTooLong) => {
            problem = Some(format!(
                "'{}' took too long to match \u{2014} try a simpler expression.",
                source.name()
            ));
        }
    }

    let mut built = String::new();
    for segment in segments {
        match segment.part {
            RenamePart::Literal => built.push_str(&segment.argument),
            RenamePart::Name => {
                built.push_str(&stem);
                built.push_str(&extension);
            }
            RenamePart::Base => built.push_str(&stem),
            RenamePart::Extension => built.push_str(&extension),
            RenamePart::Parent => built.push_str(&parent_name(&source.path)),
            RenamePart::Counter => {
                // Padded with the sign outside, so a negative step gives -001 and not 0-1.
                let value =
                    i64::from(rule.counter_start) + index as i64 * i64::from(rule.counter_step);
                built.push_str(&padded(value, segment.argument.len()));
            }
            RenamePart::Modified => {
                problem = date(&mut built, source, &segment.argument).or(problem);
            }
            RenamePart::Tag => problem = tag(&mut built, source, segment).or(problem),
        }
    }

    RenamedName {
        name: clean(&built),
        problem,
    }
}

/// Puts one of the file's own metadata fields into its name, or says why it cannot.
///
/// **A missing tag is a refusal for that item, never an empty piece of a name.** "{artist} -
/// {title}" over a file with no artist would otherwise come out as " - Song", and forty such
/// files would be forty names nobody chose. And what a tag holds is somebody else's text: a title
/// can contain a slash or a colon, so anything a name cannot hold becomes an underscore here rather
/// than failing the whole name on a character the user never typed.
fn tag(built: &mut String, source: &RenameSource, segment: &RenameSegment) -> Option<String> {
    let (field, argument) = RenameTemplate::tag_of(segment);
    let label = field.label.to_lowercase();

    let value = match source.tags.as_ref().and_then(|t| t.get(&field.field)) {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Some(format!(
                "'{}' has no {label} to put in its name.",
                source.name()
            ));
        }
    };

    match field.kind {
        MetadataFieldKind::Number if parse_unsigned(value).is_some() => {
            let number = parse_unsigned(value).expect("checked");
            built.push_str(&padded(i64::from(number), argument.len()));
            None
        }
        MetadataFieldKind::Date if MetadataFields::try_parse_date(value).is_some() => {
            let date = MetadataFields::try_parse_date(value).expect("checked");
            let format = if argument.is_empty() {
                RenamePattern::DEFAULT_DATE_FORMAT
            } else {
                argument
            };
            match format_date(date, format) {
                Ok(text) => {
                    built.push_str(&text);
                    None
                }
                Err(_) => Some(format!(
                    "'{argument}' is not a date format. Try {}.",
                    RenamePattern::DEFAULT_DATE_FORMAT
                )),
            }
        }
        _ => {
            for c in value.trim().chars() {
                built.push(if c.is_control() || INVALID_CHARS.contains(&c) {
                    '_'
                } else {
                    c
                });
            }
            None
        }
    }
}

/// `int.TryParse(value, NumberStyles.None)`: digits only, fitting an int.
fn parse_unsigned(value: &str) -> Option<i32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn date(built: &mut String, source: &RenameSource, format: &str) -> Option<String> {
    let Some(date) = source.modified else {
        return Some(format!(
            "'{}' has no modified date to put in its name.",
            source.name()
        ));
    };
    let chosen = if format.is_empty() {
        RenamePattern::DEFAULT_DATE_FORMAT
    } else {
        format
    };
    match format_date(date, chosen) {
        Ok(text) => {
            built.push_str(&text);
            None
        }
        Err(_) => Some(format!(
            "'{format}' is not a date format. Try {}.",
            RenamePattern::DEFAULT_DATE_FORMAT
        )),
    }
}

/// Applies the find/replace and the case transform to whichever part of the existing name the
/// rule's scope names, and hands back the two halves the template draws on.
fn rewrite(
    stem: &str,
    extension: &str,
    is_directory: bool,
    rule: &RenameRule,
    regex: Option<&Regex>,
) -> Result<(String, String), regex_replace::MatchTooLong> {
    Ok(match rule.scope {
        RenameScope::Extension => (stem.to_string(), convert(extension, rule, regex)?),
        RenameScope::WholeName => {
            // Re-split, so {base} and {ext} still mean something after a replace that moved the
            // dot — or removed it.
            let whole = convert(&format!("{stem}{extension}"), rule, regex)?;
            let (s, e) = split_name(&whole, is_directory);
            (s.to_string(), e.to_string())
        }
        RenameScope::Stem => {
            // A replace routinely leaves a trailing space behind ("report v2" losing "v2"), and
            // the final clean cannot reach it: by then the name ends in ".txt".
            (
                convert(stem, rule, regex)?.trim().to_string(),
                extension.to_string(),
            )
        }
    })
}

fn convert(
    value: &str,
    rule: &RenameRule,
    regex: Option<&Regex>,
) -> Result<String, regex_replace::MatchTooLong> {
    let mut value = value.to_string();
    if !rule.find.is_empty() {
        value = match regex {
            Some(regex) => regex_replace::replace_all(regex, &value, &rule.replace)?,
            None => replace_plain(&value, &rule.find, &rule.replace, rule.match_case),
        };
    }
    Ok(recase(&value, rule.case))
}

/// `string.Replace(find, replace, comparison)`: ordinal, or ordinal-ignore-case by the simple
/// per-character fold `PathKey` keeps.
fn replace_plain(value: &str, find: &str, replace: &str, match_case: bool) -> String {
    if match_case {
        return value.replace(find, replace);
    }
    let haystack: Vec<char> = value.chars().collect();
    let needle: Vec<char> = find.chars().collect();
    let same = |a: char, b: char| upper_simple(a) == upper_simple(b);
    let mut out = String::new();
    let mut i = 0;
    while i < haystack.len() {
        if i + needle.len() <= haystack.len()
            && needle
                .iter()
                .enumerate()
                .all(|(k, &n)| same(haystack[i + k], n))
        {
            out.push_str(replace);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Re-cases a name, always invariantly.
///
/// The culture matters more than it looks. Under tr-TR, `"FILE".ToLower()` was "fıle" with a
/// dotless ı — a different name, across the whole batch, on somebody else's machine. The mapping
/// here is the simple per-character one `PathKey` keeps, for the same reason.
///
/// Title case lower-cases first because `ToTitleCase` left an already-upper-case word exactly as
/// it found it, and an all-caps name is the main reason anyone reaches for it — "HOLIDAY PHOTO"
/// would otherwise come back unchanged and read as a dead button.
fn recase(value: &str, kind: RenameCase) -> String {
    match kind {
        RenameCase::AsIs => value.to_string(),
        RenameCase::Lower => value.chars().map(lower_simple).collect(),
        RenameCase::Upper => value.chars().map(upper_simple).collect(),
        RenameCase::Title => title(&value.chars().map(lower_simple).collect::<String>()),
        RenameCase::Sentence => sentence(value),
    }
}

/// `TextInfo.ToTitleCase`: the first letter of each word up, where a word runs from a letter over
/// letters, digits and apostrophes until a separator (space, punctuation, symbol).
fn title(lowered: &str) -> String {
    let mut out = String::with_capacity(lowered.len());
    let mut in_word = false;
    for c in lowered.chars() {
        if !in_word && c.is_alphabetic() {
            out.push(upper_simple(c));
            in_word = true;
        } else {
            out.push(c);
            in_word = c.is_alphanumeric() || c == '\'';
        }
    }
    out
}

fn sentence(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut done = false;
    for c in value.chars().map(lower_simple) {
        if !done && c.is_alphabetic() {
            out.push(upper_simple(c));
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// `char.ToUpperInvariant`: one character to one character, never `ß` to `SS`.
fn upper_simple(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// `char.ToLowerInvariant`.
fn lower_simple(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// `value.ToString("D<width>")`: zero-padded to `width` digits with the sign outside.
fn padded(value: i64, width: usize) -> String {
    let digits = format!("{:0width$}", value.unsigned_abs());
    if value < 0 {
        format!("-{digits}")
    } else {
        digits
    }
}

/// Every item left at the name it already has, carrying one rule-level problem — what a template
/// or an expression that cannot be used at all produces.
fn everything(sources: &[RenameSource], problem: Option<String>) -> Vec<RenamedName> {
    sources
        .iter()
        .map(|s| RenamedName {
            name: s.name().to_string(),
            problem: problem.clone(),
        })
        .collect()
}

/// The containing folder's name — "Photos", never "C:\Users\Rob\Photos". A drive root has no name
/// of its own, so it gives up its letter instead of an empty string.
fn parent_name(path: &str) -> String {
    let Some(parent) = directory_name(trim_ending_separator(path)) else {
        return String::new();
    };
    let trimmed = trim_ending_separator(&parent);
    let name = file_name(trimmed);
    if name.is_empty() {
        trimmed.trim_end_matches(':').to_string()
    } else {
        name.to_string()
    }
}

/// A date that exercises every field of a format string without being ambiguous.
fn sample_date() -> FileTime {
    FileTime::from_ymd_hms(2026, 8, 28, 13, 45, 7)
}

/// Trailing spaces and periods are silently dropped by Windows, so a name that only differs by them
/// is not a rename at all; take them off before anything sees the name.
fn clean(pattern: &str) -> String {
    pattern.trim().trim_end_matches(['.', ' ']).to_string()
}

fn split_name(name: &str, is_directory: bool) -> (&str, &str) {
    if is_directory {
        return (name, "");
    }
    let (stem, extension) = split_extension(name);
    if stem.is_empty() {
        (name, "")
    } else {
        (stem, extension)
    }
}
