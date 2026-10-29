//! The wildcard vocabulary shared by the in-memory matcher and the SQL it compiles to, so "what does
//! `*` mean" has one answer. Spec: Core/Services/Search/GlobText.cs.

pub struct GlobText;

impl GlobText {
    /// Escapes a run of text for SQLite GLOB. `[` opens a character class and is the one character
    /// that always needs escaping (`[[]` is a class matching a literal `[`); a bare `]` outside a
    /// class is already literal. When `keep_wildcards` is false (a quoted phrase), `*` and `?` are
    /// escaped as classes too.
    pub fn escape(term: &str, keep_wildcards: bool) -> String {
        let mut out = String::with_capacity(term.len() + 8);
        Self::escape_into(&mut out, term, keep_wildcards);
        out
    }

    /// [`escape`](Self::escape) appended to `out`, for a caller building a larger string.
    pub fn escape_into(out: &mut String, term: &str, keep_wildcards: bool) {
        let needs_work =
            term.contains('[') || (!keep_wildcards && (term.contains('*') || term.contains('?')));
        if !needs_work {
            out.push_str(term);
            return;
        }
        for c in term.chars() {
            if c == '[' {
                out.push_str("[[]");
            } else if !keep_wildcards && (c == '*' || c == '?') {
                out.push('[');
                out.push(c);
                out.push(']');
            } else {
                out.push(c);
            }
        }
    }

    /// Classic two-pointer wildcard match with star backtracking, over characters. No regex: this
    /// runs once per term for every entry a live scan visits.
    pub fn wildcard_match(s: &str, p: &str) -> bool {
        let s = s.as_bytes();
        let p = p.as_bytes();
        let (mut si, mut pi) = (0usize, 0usize);
        let mut star: Option<usize> = None;
        let mut matched = 0usize;
        while si < s.len() {
            if pi < p.len() && p[pi] == b'?' {
                si += char_len(s[si]);
                pi += 1;
            } else if pi < p.len() && p[pi] != b'*' && p[pi] == s[si] {
                si += 1;
                pi += 1;
            } else if pi < p.len() && p[pi] == b'*' {
                star = Some(pi);
                pi += 1;
                matched = si;
            } else if let Some(st) = star {
                pi = st + 1;
                matched += char_len(s[matched]);
                si = matched;
            } else {
                return false;
            }
        }
        while pi < p.len() && p[pi] == b'*' {
            pi += 1;
        }
        pi == p.len()
    }
}

/// The byte length of the UTF-8 character starting with `lead`.
fn char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}
