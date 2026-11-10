//! `Regex.Replace(input, replacement)` with .NET's replacement spellings over `fancy-regex`:
//! `$1`, `${1}`, `${name}`, `$0` and `$$` (docs/regex-dialect.md). The engine's own
//! expansion would read `$2_$1` as a group named `2_`, so the expansion is done here, with .NET's
//! rules: the longest run of digits that names a real group, and a `$` that names nothing kept
//! literally.

use fancy_regex::{Regex, RegexBuilder};

/// Why one name could not be matched: the pattern hit the backtrack limit, which is this engine's
/// "took too long" — deterministic, where a wall-clock deadline was not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchTooLong;

/// The same ceiling the search grammar gives a `regex:` term.
const BACKTRACK_LIMIT: usize = 200_000;

/// Compiles `find`, case-insensitively unless `match_case`, or says why it will not compile.
pub fn build(find: &str, match_case: bool) -> Result<Regex, String> {
    RegexBuilder::new(find)
        .case_insensitive(!match_case)
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
        .map_err(|e| e.to_string())
}

enum Piece {
    Literal(String),
    Group(usize),
}

/// Every match in `text` replaced by `replacement`, expanded.
pub fn replace_all(regex: &Regex, text: &str, replacement: &str) -> Result<String, MatchTooLong> {
    let pieces = parse_replacement(regex, replacement);
    let mut out = String::new();
    let mut last = 0;
    for captures in regex.captures_iter(text) {
        let captures = captures.map_err(|_| MatchTooLong)?;
        let whole = captures.get(0).expect("group 0 is the match");
        out.push_str(&text[last..whole.start()]);
        for piece in &pieces {
            match piece {
                Piece::Literal(s) => out.push_str(s),
                Piece::Group(n) => {
                    if let Some(m) = captures.get(*n) {
                        out.push_str(m.as_str());
                    }
                }
            }
        }
        last = whole.end();
    }
    out.push_str(&text[last..]);
    Ok(out)
}

fn parse_replacement(regex: &Regex, replacement: &str) -> Vec<Piece> {
    let group_count = regex.captures_len();
    let names: Vec<Option<&str>> = regex.capture_names().collect();
    let chars: Vec<char> = replacement.chars().collect();
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut i = 0;

    let flush = |literal: &mut String, pieces: &mut Vec<Piece>| {
        if !literal.is_empty() {
            pieces.push(Piece::Literal(std::mem::take(literal)));
        }
    };

    while i < chars.len() {
        if chars[i] != '$' {
            literal.push(chars[i]);
            i += 1;
            continue;
        }
        match chars.get(i + 1) {
            Some('$') => {
                literal.push('$');
                i += 2;
            }
            Some('{') => {
                let Some(close) = chars[i + 2..].iter().position(|&c| c == '}') else {
                    literal.push('$');
                    i += 1;
                    continue;
                };
                let body: String = chars[i + 2..i + 2 + close].iter().collect();
                let group = if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) {
                    body.parse::<usize>().ok().filter(|&n| n < group_count)
                } else {
                    names.iter().position(|n| *n == Some(body.as_str()))
                };
                match group {
                    Some(n) => {
                        flush(&mut literal, &mut pieces);
                        pieces.push(Piece::Group(n));
                    }
                    None => literal.push_str(&format!("${{{body}}}")),
                }
                i += close + 3;
            }
            Some(c) if c.is_ascii_digit() => {
                let digits: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                // The longest prefix that names a real group, as .NET backed off to.
                let mut taken = digits.len();
                let mut group = None;
                while taken > 0 {
                    if let Ok(n) = digits[..taken].parse::<usize>()
                        && n < group_count
                    {
                        group = Some(n);
                        break;
                    }
                    taken -= 1;
                }
                match group {
                    Some(n) => {
                        flush(&mut literal, &mut pieces);
                        pieces.push(Piece::Group(n));
                        i += 1 + taken;
                    }
                    None => {
                        literal.push('$');
                        i += 1;
                    }
                }
            }
            _ => {
                literal.push('$');
                i += 1;
            }
        }
    }
    flush(&mut literal, &mut pieces);
    pieces
}
