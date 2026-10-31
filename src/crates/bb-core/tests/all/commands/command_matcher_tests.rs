// Port of tests/BertBrowser.Core.Tests/CommandMatcherTests.cs.
use bb_core::commands::{CommandMatch, CommandMatcher, MatchTier};

fn matches(query: &str, name: &str) -> CommandMatch {
    CommandMatcher::matches(query, name, &[], "Tabs", "")
}

fn with(query: &str, name: &str, gesture: &str, aliases: &[&str]) -> CommandMatch {
    CommandMatcher::matches(query, name, aliases, "Tabs", gesture)
}

fn marked(query: &str, name: &str) -> String {
    let m = matches(query, name);
    let mut chars: Vec<String> = name.chars().map(|c| c.to_string()).collect();
    let mut ranges = m.highlights.clone();
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    for range in ranges {
        chars[range.start + range.length - 1].push(']');
        chars[range.start].insert(0, '[');
    }
    chars.concat()
}

#[test]
fn where_the_words_were_found_decides_the_tier() {
    for (query, name, tier) in [
        ("new", "New tab", MatchTier::Start),
        ("NEW T", "New tab", MatchTier::Word),
        ("tab", "New tab", MatchTier::Word),
        ("nt", "New tab", MatchTier::Word),
        ("ew", "New tab", MatchTier::Word),
        ("nwtb", "New tab", MatchTier::Loose),
        ("tabs", "New tab", MatchTier::Loose),
        ("xyz", "New tab", MatchTier::None),
    ] {
        assert_eq!(tier, matches(query, name).tier, "{query} / {name}");
    }
}

#[test]
fn every_word_typed_has_to_be_found() {
    assert!(matches("close tab", "Close tab").is_match());
    assert!(matches("tab close", "Close tab").is_match());
    assert!(!matches("close pane", "Close tab").is_match());
}

#[test]
fn a_match_is_only_as_strong_as_its_weakest_word() {
    assert_eq!(MatchTier::Start, matches("close", "Close other tabs").tier);
    assert_eq!(
        MatchTier::Word,
        matches("close tabs", "Close other tabs").tier
    );
}

#[test]
fn a_competitors_word_for_it_finds_it_but_ranks_under_its_own_name() {
    let by_alias = with(
        "branch",
        "Toggle flat view",
        "",
        &["branch view", "flatten"],
    );
    let by_name = with("flat", "Toggle flat view", "", &["branch view", "flatten"]);
    assert_eq!(MatchTier::Loose, by_alias.tier);
    assert!(by_alias.highlights.is_empty());
    assert!(by_name.tier > by_alias.tier);
}

#[test]
fn a_shortcut_typed_whole_is_a_lookup() {
    assert_eq!(
        MatchTier::Start,
        with("ctrl+b", "Toggle flat view", "Ctrl+B", &[]).tier
    );
    assert_eq!(
        MatchTier::Start,
        with("Ctrl + B", "Toggle flat view", "Ctrl+B", &[]).tier
    );
    assert_eq!(
        MatchTier::Start,
        with("f7", "Compare with other pane", "F7", &[]).tier
    );
    assert!(!with("ctrl+b", "New tab", "Ctrl+T", &[]).is_match());
}

#[test]
fn part_of_a_shortcut_lists_what_is_on_it() {
    assert!(with("ctrl+shift", "Search this PC", "Ctrl+Shift+F", &[]).is_match());
    assert!(!with("ctrl+shift", "New tab", "Ctrl+T", &[]).is_match());
}

#[test]
fn two_letters_in_order_are_not_a_match_by_themselves() {
    // "nb" is in order in New tab, and in order in nearly everything else too.
    assert!(!matches("nb", "New tab").is_match());
}

#[test]
fn a_shorter_name_is_the_closer_match_for_the_same_word() {
    assert!(matches("copy", "Copy").score > matches("copy", "Copy as path").score);
}

#[test]
fn letters_sitting_together_beat_the_same_letters_scattered() {
    assert!(matches("dup", "Find duplicates").score > matches("dpl", "Find duplicates").score);
}

#[test]
fn the_name_is_marked_where_it_matched() {
    for (query, name, expected) in [
        ("new", "New tab", "[New] tab"),
        ("tab", "New tab", "New [tab]"),
        ("nt", "New tab", "[N]ew [t]ab"),
        ("new tab", "New tab", "[New] [tab]"),
        ("pth", "Copy as path", "Copy as [p]a[th]"),
    ] {
        assert_eq!(expected, marked(query, name), "{query}");
    }
}

#[test]
fn nothing_typed_matches_nothing() {
    assert!(!matches("", "New tab").is_match());
    assert!(!matches("   ", "New tab").is_match());
}
