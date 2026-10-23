//! Access keys in captions. A caption marks its key the way Windows always has — `&File`, with
//! `&&` for a real ampersand — and anything that comes from data (a workspace's name, a command's
//! own name) is `escape`d before it gets here, so "R&D" does not become a menu answering to D.
//! Spec: Core/Services/CommandBars/AccessKeys.cs.

use std::collections::HashSet;

pub struct AccessKeys;

impl AccessKeys {
    /// Text that is not a caption, made safe to use as one.
    pub fn escape(text: &str) -> String {
        text.replace('&', "&&")
    }

    /// The caption as plain words.
    pub fn strip(caption: &str) -> String {
        let chars: Vec<char> = caption.chars().collect();
        let mut plain = String::with_capacity(caption.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '&' && i + 1 < chars.len() {
                i += 1;
            }
            plain.push(chars[i]);
            i += 1;
        }
        plain
    }

    /// The key a caption answers to, uppercased, or None when it marks none.
    pub fn key_of(caption: &str) -> Option<char> {
        let chars: Vec<char> = caption.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == '&' {
                if chars[i + 1] != '&' {
                    return Some(upper(chars[i + 1]));
                }
                i += 1;
            }
            i += 1;
        }
        None
    }

    /// Gives every caption of one menu a key of its own where it can. A key a caption already
    /// marks is kept — and wins over any chosen here — then each remaining caption takes the first
    /// free letter among its word initials, then among all its letters. One that finds none goes
    /// without rather than share.
    pub fn assign<S: AsRef<str>>(captions: &[S]) -> Vec<String> {
        let mut taken: HashSet<char> = captions
            .iter()
            .filter_map(|c| Self::key_of(c.as_ref()))
            .collect();

        captions
            .iter()
            .map(|c| {
                let caption = c.as_ref();
                if Self::key_of(caption).is_none()
                    && let Some(at) = choose(caption, &mut taken)
                {
                    let mut marked = String::with_capacity(caption.len() + 1);
                    for (i, ch) in caption.chars().enumerate() {
                        if i == at {
                            marked.push('&');
                        }
                        marked.push(ch);
                    }
                    marked
                } else {
                    caption.to_string()
                }
            })
            .collect()
    }

    /// The caption in the spelling WPF read: an underscore before the key, a doubled underscore for
    /// a real one. Kept here, beside the rule it translates, so it can be tested without a window.
    pub fn to_underscored(caption: &str) -> String {
        let chars: Vec<char> = caption.chars().collect();
        let mut text = String::with_capacity(caption.len() + 2);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '_' {
                text.push_str("__");
            } else if c == '&' && i + 1 < chars.len() {
                i += 1;
                if chars[i] == '&' {
                    text.push('&');
                } else {
                    text.push('_');
                    text.push(if chars[i] == '_' { '_' } else { chars[i] });
                }
            } else {
                text.push(c);
            }
            i += 1;
        }
        text
    }
}

/// Where to put the mark (a char index), claiming the key; None when every candidate is taken.
fn choose(caption: &str, taken: &mut HashSet<char>) -> Option<usize> {
    let chars: Vec<char> = caption.chars().collect();
    // Word initials first: "Date modified" would rather answer to D or M than to A.
    for initials_only in [true, false] {
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '&' {
                i += 2;
                continue;
            }
            if c.is_alphanumeric()
                && !(initials_only && i > 0 && chars[i - 1].is_alphanumeric())
                && taken.insert(upper(c))
            {
                return Some(i);
            }
            i += 1;
        }
    }
    None
}

/// One character to one character, as `ToUpperInvariant` did.
fn upper(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}
