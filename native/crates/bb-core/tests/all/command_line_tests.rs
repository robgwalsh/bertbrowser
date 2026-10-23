// Port of tests/BertBrowser.Core.Tests/CommandLineTests.cs.
use bb_core::cli::{CommandLine, CommandLineRequest, OpenIn};

fn parse(args: &[&str]) -> CommandLineRequest {
    CommandLine::parse(args)
}

// --- the ordinary cases ---

#[test]
fn no_arguments_asks_for_nothing() {
    let request = parse(&[]);
    assert!(!request.has_targets());
    assert_eq!(OpenIn::Default, request.mode);
    assert!(request.errors.is_empty());
}

#[test]
fn a_bare_directory_is_the_target() {
    let request = parse(&[r"C:\Dir"]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Dir", request.targets[0].path);
    assert!(!request.targets[0].select);
}

#[test]
fn several_paths_keep_their_order() {
    let request = parse(&[r"C:\One", r"C:\Two", r"C:\Three"]);
    let paths: Vec<&str> = request.targets.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(vec![r"C:\One", r"C:\Two", r"C:\Three"], paths);
}

#[test]
fn a_unc_path_is_a_target_like_any_other() {
    let request = parse(&[r"\\server\share"]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"\\server\share", request.targets[0].path);
}

// --- Explorer's /select ---

/// Explorer emits this as one token when no space follows the comma.
#[test]
fn select_in_one_token_highlights_the_item() {
    let request = parse(&[r"/select,C:\Dir\file.txt"]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Dir\file.txt", request.targets[0].path);
    assert!(request.targets[0].select);
}

/// ...and as two when one does.
#[test]
fn select_split_across_two_tokens_highlights_the_item() {
    let request = parse(&["/select,", r"C:\Dir\file.txt"]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Dir\file.txt", request.targets[0].path);
    assert!(request.targets[0].select);
}

#[test]
fn the_long_form_of_select_works_too() {
    let request = parse(&["--select", r"C:\Dir\file.txt"]);
    assert_eq!(1, request.targets.len());
    assert!(request.targets[0].select);
}

#[test]
fn select_applies_to_the_next_path_only() {
    let request = parse(&["--select", r"C:\Dir\file.txt", r"C:\Other"]);
    assert!(request.targets[0].select);
    assert!(!request.targets[1].select);
}

#[test]
fn select_with_nothing_after_it_is_reported() {
    let request = parse(&["--select"]);
    assert!(request.targets.is_empty());
    assert_eq!(1, request.errors.len());
}

#[test]
fn select_is_case_insensitive_because_explorer_is_not_consistent() {
    let request = parse(&[r"/SELECT,C:\Dir\file.txt"]);
    assert_eq!(1, request.targets.len());
    assert!(request.targets[0].select);
}

// --- where it opens ---

#[test]
fn new_tab_is_requestable() {
    for flag in ["--new-tab", "-t"] {
        assert_eq!(OpenIn::NewTab, parse(&[flag]).mode, "{flag}");
    }
}

#[test]
fn new_pane_is_requestable() {
    for flag in ["--new-pane", "-p"] {
        assert_eq!(OpenIn::NewPane, parse(&[flag]).mode, "{flag}");
    }
}

/// Silently picking one of two contradictory flags looks exactly like the other one being ignored.
#[test]
fn asking_for_both_a_tab_and_a_pane_is_an_error_rather_than_a_coin_flip() {
    assert_eq!(
        1,
        parse(&["--new-tab", "--new-pane", r"C:\Dir"]).errors.len()
    );
}

/// The error names the two flags that actually exist; this app splits panes, it does not open windows.
#[test]
fn the_contradiction_error_names_the_real_flags() {
    let request = parse(&["--new-tab", "--new-pane", r"C:\Dir"]);
    assert_eq!(1, request.errors.len());
    assert!(request.errors[0].contains("--new-pane"));
    assert!(!request.errors[0].contains("--new-window"));
}

#[test]
fn repeating_the_same_flag_is_not_a_contradiction() {
    let request = parse(&["--new-tab", "-t", r"C:\Dir"]);
    assert!(request.errors.is_empty());
    assert_eq!(OpenIn::NewTab, request.mode);
}

#[test]
fn flags_may_come_after_paths() {
    let request = parse(&[r"C:\Dir", "--new-tab"]);
    assert_eq!(OpenIn::NewTab, request.mode);
    assert_eq!(1, request.targets.len());
}

// --- what must never happen ---

/// A mistyped flag opening the user's profile folder is worse than an error message.
#[test]
fn an_unrecognized_option_is_reported_never_treated_as_a_path() {
    for arg in ["--nwe-tab", "--help", "/x", "-"] {
        let request = parse(&[arg]);
        assert!(request.targets.is_empty(), "{arg}");
        assert_eq!(1, request.errors.len(), "{arg}");
    }
}

#[test]
fn an_unrecognized_option_does_not_cost_the_valid_paths_beside_it() {
    let request = parse(&["--nope", r"C:\Dir"]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Dir", request.targets[0].path);
    assert_eq!(1, request.errors.len());
}

/// The one everybody hits: `"C:\Dir\"` reaches argv as `C:\Dir"`.
#[test]
fn a_trailing_separator_mangled_into_a_quote_is_repaired() {
    let request = parse(&["C:\\Dir\""]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Dir", request.targets[0].path);
}

#[test]
fn the_same_mangling_is_repaired_after_select() {
    let request = parse(&["/select,C:\\Program Files\""]);
    assert_eq!(1, request.targets.len());
    assert_eq!(r"C:\Program Files", request.targets[0].path);
}

#[test]
fn blank_arguments_are_ignored() {
    let request = parse(&["", "   ", r"C:\Dir"]);
    assert_eq!(1, request.targets.len());
    assert!(request.errors.is_empty());
}
