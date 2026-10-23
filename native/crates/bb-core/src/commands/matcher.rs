//! Matches typed text against a command the way a palette has to: forgiving about what was typed,
//! strict about order of preference. Every word typed must be found somewhere, and where it was found
//! decides the rank. The preference, strongest first: the name starts with the word; a word of the
//! name starts with it; the initials spell it; the name contains it; an alias matches; the category
//! matches; the letters appear in order. Spec: Core/Services/Commands/CommandMatcher.cs.

/// How well typed text matched a command, strongest last when sorted ascending.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MatchTier {
    None = 0,
    /// Found only by its letters in order, by another word for it, by its category or by its shortcut.
    Loose = 1,
    /// A word of the name starts with what was typed, or the name contains it.
    Word = 2,
    /// The name starts with what was typed.
    Start = 3,
}

/// A run of characters in a name to draw as matched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchRange {
    pub start: usize,
    pub length: usize,
}

impl MatchRange {
    pub fn new(start: usize, length: usize) -> MatchRange {
        MatchRange { start, length }
    }
}

/// The verdict on one command for one piece of typed text. `highlights` are where in the *name* the
/// text matched; empty when it matched by an alias, the category or the shortcut.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommandMatch {
    pub tier: MatchTier,
    pub score: i32,
    pub highlights: Vec<MatchRange>,
}

impl CommandMatch {
    pub fn none() -> CommandMatch {
        CommandMatch {
            tier: MatchTier::None,
            score: 0,
            highlights: Vec::new(),
        }
    }

    fn at(tier: MatchTier, score: i32) -> CommandMatch {
        CommandMatch {
            tier,
            score,
            highlights: Vec::new(),
        }
    }

    pub fn is_match(&self) -> bool {
        self.tier != MatchTier::None
    }
}

pub struct CommandMatcher;

impl CommandMatcher {
    pub fn matches(
        query: &str,
        name: &str,
        aliases: &[&str],
        category: &str,
        gesture: &str,
    ) -> CommandMatch {
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return CommandMatch::none();
        }
        // A shortcut typed whole ("ctrl+b", "f7") is a lookup, not a search.
        let typed: String = words.concat();
        if !gesture.is_empty() && same(&typed, &gesture.replace(' ', "")) {
            return CommandMatch::at(MatchTier::Start, 1000);
        }

        let name_chars: Vec<char> = name.chars().collect();
        let mut tier = MatchTier::Start;
        let mut score = 0;
        let mut highlights = Vec::new();
        for word in words {
            let word_chars: Vec<char> = word.chars().collect();
            let mut best = in_name(&word_chars, &name_chars, &mut highlights);
            if !best.is_match() {
                best = elsewhere(word, aliases, category, gesture);
            }
            if !best.is_match() {
                best = subsequence(&word_chars, &name_chars, &mut highlights);
            }
            if !best.is_match() {
                return CommandMatch::none();
            }
            if best.tier < tier {
                tier = best.tier;
            }
            score += best.score;
        }
        // A shorter name is the closer match for the same words: "Copy" before "Copy as path".
        score -= (name_chars.len().min(40) / 4) as i32;
        CommandMatch {
            tier,
            score,
            highlights: merge(highlights),
        }
    }
}

fn fold(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn eq_at(name: &[char], start: usize, word: &[char]) -> bool {
    start + word.len() <= name.len()
        && word
            .iter()
            .enumerate()
            .all(|(i, &w)| fold(name[start + i]) == fold(w))
}

fn in_name(word: &[char], name: &[char], highlights: &mut Vec<MatchRange>) -> CommandMatch {
    if eq_at(name, 0, word) {
        highlights.push(MatchRange::new(0, word.len()));
        return CommandMatch::at(MatchTier::Start, 100);
    }
    for start in word_starts(name) {
        if start == 0 || !eq_at(name, start, word) {
            continue;
        }
        highlights.push(MatchRange::new(start, word.len()));
        return CommandMatch::at(MatchTier::Word, 80);
    }
    if word.len() >= 2 && initials(word, name, highlights) {
        return CommandMatch::at(MatchTier::Word, 70);
    }
    if let Some(at) = (0..name.len()).find(|&i| eq_at(name, i, word)) {
        highlights.push(MatchRange::new(at, word.len()));
        return CommandMatch::at(MatchTier::Word, 60);
    }
    CommandMatch::none()
}

fn elsewhere(word: &str, aliases: &[&str], category: &str, gesture: &str) -> CommandMatch {
    let lower = word.to_lowercase();
    if aliases.iter().any(|a| a.to_lowercase().starts_with(&lower)) {
        return CommandMatch::at(MatchTier::Loose, 50);
    }
    if aliases.iter().any(|a| a.to_lowercase().contains(&lower)) {
        return CommandMatch::at(MatchTier::Loose, 40);
    }
    if category.to_lowercase().starts_with(&lower) {
        return CommandMatch::at(MatchTier::Loose, 35);
    }
    // Part of a shortcut: "ctrl+shift" lists everything on that pair of modifiers.
    if word.contains('+') && gesture.to_lowercase().contains(&lower) {
        return CommandMatch::at(MatchTier::Loose, 30);
    }
    CommandMatch::none()
}

/// The word's letters, in order, anywhere in the name. Needs three letters: with two, nearly every
/// pair of letters is "in order" in something. Tried from every place the first letter occurs,
/// keeping the tightest run.
fn subsequence(word: &[char], name: &[char], highlights: &mut Vec<MatchRange>) -> CommandMatch {
    if word.len() < 3 {
        return CommandMatch::none();
    }
    let index_of =
        |letter: char, from: usize| (from..name.len()).find(|&i| fold(name[i]) == fold(letter));
    let mut best: Option<Vec<usize>> = None;
    let mut from = index_of(word[0], 0);
    while let Some(start) = from {
        let mut run = vec![start];
        let mut at = start + 1;
        let mut complete = true;
        for &letter in &word[1..] {
            match index_of(letter, at) {
                Some(next) => {
                    run.push(next);
                    at = next + 1;
                }
                None => {
                    complete = false;
                    break;
                }
            }
        }
        // Later starts only have less of the name left, so the first failure is the last try.
        if !complete {
            break;
        }
        let spread = run[run.len() - 1] - run[0];
        if best.as_ref().is_none_or(|b| spread < b[b.len() - 1] - b[0]) {
            best = Some(run);
        }
        from = index_of(word[0], start + 1);
    }
    let Some(best) = best else {
        return CommandMatch::none();
    };
    let spread = (best[best.len() - 1] - best[0] + 1 - word.len()) as i32;
    highlights.extend(best.iter().map(|&i| MatchRange::new(i, 1)));
    CommandMatch::at(MatchTier::Loose, (30 - spread).max(5))
}

/// "nt" for New tab, "cap" for Copy as path: one letter per word, from the first.
fn initials(word: &[char], name: &[char], highlights: &mut Vec<MatchRange>) -> bool {
    let starts: Vec<usize> = word_starts(name).collect();
    if starts.len() < word.len() {
        return false;
    }
    if !word
        .iter()
        .enumerate()
        .all(|(i, &w)| fold(name[starts[i]]) == fold(w))
    {
        return false;
    }
    highlights.extend(
        starts
            .iter()
            .take(word.len())
            .map(|&s| MatchRange::new(s, 1)),
    );
    true
}

fn word_starts(name: &[char]) -> impl Iterator<Item = usize> + '_ {
    (0..name.len())
        .filter(move |&i| name[i].is_alphanumeric() && (i == 0 || !name[i - 1].is_alphanumeric()))
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Sorted, with touching and overlapping runs joined, so the view draws each stretch once.
fn merge(mut ranges: Vec<MatchRange>) -> Vec<MatchRange> {
    if ranges.len() < 2 {
        return ranges;
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<MatchRange> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && last.start + last.length >= range.start
        {
            let end = (last.start + last.length).max(range.start + range.length);
            last.length = end - last.start;
        } else {
            merged.push(range);
        }
    }
    merged
}
