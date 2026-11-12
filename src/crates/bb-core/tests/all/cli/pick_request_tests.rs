//! A request to choose a file, from the command line and over the single-instance pipe. Everything
//! in one arrives from another program, and the reply pipe is a name this app goes on to open and
//! write the user's choice to, so what is refused matters as much as what is read.

use bb_core::cli::{CommandLine, CommandLineRequest, NavigationRequest, PickMode, PickRequest};

fn parse(args: &[&str]) -> CommandLineRequest {
    CommandLine::parse(args)
}

fn pick(args: &[&str]) -> PickRequest {
    let request = parse(args);
    assert_eq!(Vec::<String>::new(), request.errors);
    request.pick.expect("a pick request")
}

// --- the command line ---

#[test]
fn a_pick_to_open_carries_everything_it_was_given() {
    let p = pick(&[
        "--pick",
        "open",
        "--reply-pipe",
        "bertcut-pick-4242-1a2b3c4d",
        "--pick-title",
        "Add a video",
        "--pick-filter",
        "*.mp4;*.mkv",
        r"C:\clips",
    ]);
    assert_eq!(PickMode::Open, p.mode);
    assert_eq!("bertcut-pick-4242-1a2b3c4d", p.reply_pipe);
    assert_eq!("Add a video", p.title);
    assert_eq!("*.mp4;*.mkv", p.filter);
    assert_eq!("", p.default_name);
    assert_eq!(r"C:\clips", p.start);
}

#[test]
fn a_pick_to_save_carries_the_name_on_offer() {
    let p = pick(&[
        "--pick",
        "SAVE",
        "--reply-pipe",
        "p1",
        "--pick-name",
        "clip-edit.mp4",
        r"C:\clips\clip.mp4",
    ]);
    assert_eq!(PickMode::Save, p.mode);
    assert_eq!("clip-edit.mp4", p.default_name);
    assert_eq!(r"C:\clips\clip.mp4", p.start);
}

/// The path beside a pick is where to start looking. It must not also be opened as a folder.
#[test]
fn a_pick_is_the_whole_request() {
    let request = parse(&["--pick", "open", "--reply-pipe", "p1", r"C:\clips"]);
    assert!(request.pick.is_some());
    assert!(request.targets.is_empty());
}

/// A title is the asker's words and may begin with anything; it is a value, not an option.
#[test]
fn an_options_value_is_taken_whatever_it_looks_like() {
    let p = pick(&[
        "--pick",
        "open",
        "--reply-pipe",
        "p1",
        "--pick-title",
        "-- choose --",
    ]);
    assert_eq!("-- choose --", p.title);
    assert_eq!("", p.start);
}

#[test]
fn a_pick_with_no_pipe_to_answer_on_is_an_error_and_not_a_request() {
    let request = parse(&["--pick", "open", r"C:\clips"]);
    assert!(request.pick.is_none());
    assert!(request.targets.is_empty());
    assert_eq!(1, request.errors.len());
    assert!(
        request.errors[0].contains("--reply-pipe"),
        "{:?}",
        request.errors
    );
}

#[test]
fn a_pick_that_is_neither_open_nor_save_is_an_error() {
    let request = parse(&["--pick", "delete", "--reply-pipe", "p1"]);
    assert!(request.pick.is_none());
    assert!(
        request.errors[0].contains("open or save"),
        "{:?}",
        request.errors
    );
}

#[test]
fn an_option_with_nothing_after_it_says_so() {
    let request = parse(&["--pick", "open", "--reply-pipe"]);
    assert!(request.pick.is_none());
    assert!(
        request
            .errors
            .iter()
            .any(|e| e.contains("'--reply-pipe' expects a value"))
    );
}

/// The name is opened and written to. Nothing that could be read as a path, a server or a device
/// may get as far as being a request.
#[test]
fn a_reply_pipe_is_a_plain_name_and_nothing_else() {
    for hostile in [
        r"\\.\pipe\x",
        r"..\x",
        r"server\pipe\x",
        "a/b",
        "a b",
        "a\tb",
        "",
        "x:y",
        "caf\u{e9}",
    ] {
        assert!(
            !PickRequest::is_acceptable_pipe_name(hostile),
            "{hostile:?}"
        );
        let request = parse(&["--pick", "open", "--reply-pipe", hostile]);
        assert!(request.pick.is_none(), "{hostile:?}");
    }
    assert!(!PickRequest::is_acceptable_pipe_name(&"a".repeat(65)));
    assert!(PickRequest::is_acceptable_pipe_name(&"a".repeat(64)));
    assert!(PickRequest::is_acceptable_pipe_name(
        "bertcut-pick-4242-1a2b3c4d"
    ));
    assert!(PickRequest::is_acceptable_pipe_name("A_b.c-9"));
}

#[test]
fn a_default_name_is_a_file_name_and_never_a_path() {
    for bad in [
        r"..\evil.mp4",
        r"C:\x.mp4",
        "a/b.mp4",
        "..",
        ".",
        "a:b",
        "a*.mp4",
        "trailing.",
        "tab\t.mp4",
    ] {
        assert!(!PickRequest::is_acceptable_file_name(bad), "{bad:?}");
        let request = parse(&["--pick", "save", "--reply-pipe", "p1", "--pick-name", bad]);
        assert!(request.pick.is_none(), "{bad:?}");
    }
    assert!(!PickRequest::is_acceptable_file_name(" lead.mp4"));
    assert!(PickRequest::is_acceptable_file_name("clip-edit.mp4"));
    assert!(PickRequest::is_acceptable_file_name("my clip (2).gif"));
}

// --- what a filter lets through ---

fn filtered(filter: &str) -> PickRequest {
    PickRequest::new(PickMode::Open, "p1", "", filter, "", "").unwrap()
}

#[test]
fn a_filter_matches_by_extension_whatever_the_case() {
    let p = filtered("*.mp4; *.MKV");
    assert!(p.accepts("clip.mp4"));
    assert!(p.accepts("CLIP.MP4"));
    assert!(p.accepts("a.b.mkv"));
    assert!(!p.accepts("clip.txt"));
    assert!(!p.accepts("mp4"));
}

#[test]
fn no_filter_and_the_catch_all_accept_anything() {
    assert!(filtered("").accepts("anything.xyz"));
    assert!(filtered("*.*").accepts("anything.xyz"));
    assert!(filtered("*").accepts("noextension"));
}

/// A pattern this does not understand matches nothing: guessing wide would offer files the asker
/// said it could not take.
#[test]
fn a_pattern_that_is_not_understood_matches_nothing() {
    let p = filtered("clip?.mp4;*a*.mkv;exact.gif");
    assert!(!p.accepts("clip1.mp4"));
    assert!(!p.accepts("banana.mkv"));
    assert!(p.accepts("Exact.gif"));
}

// --- the wire format ---

#[test]
fn a_pick_survives_the_round_trip() {
    let original = CommandLineRequest {
        pick: Some(
            PickRequest::new(
                PickMode::Save,
                "bertcut-pick-1-abcd",
                "Save as",
                "*.mp4;*.gif",
                "clip-edit.mp4",
                r"C:\my clips",
            )
            .unwrap(),
        ),
        ..CommandLineRequest::default()
    };

    let line = NavigationRequest::format(&original);
    assert!(line.starts_with("PICK\tsave\t"), "{line}");
    assert_eq!(Some(original), NavigationRequest::try_parse(&line));
}

#[test]
fn a_pick_with_its_optional_fields_empty_still_round_trips() {
    let original = CommandLineRequest {
        pick: Some(PickRequest::new(PickMode::Open, "p1", "", "", "", "").unwrap()),
        ..CommandLineRequest::default()
    };
    assert_eq!(
        Some(original.clone()),
        NavigationRequest::try_parse(&NavigationRequest::format(&original))
    );
}

/// All or nothing: a pick with one field wrong is not a smaller pick.
#[test]
fn a_malformed_pick_line_is_no_request_at_all() {
    for line in [
        "PICK",
        "PICK\topen\tp1",
        "PICK\topen\tp1\t\t\t",
        "PICK\topen\tp1\t\t\t\t\textra",
        "PICK\tOPEN\tp1\t\t\t\t",
        "PICK\tdelete\tp1\t\t\t\t",
        "PICK\topen\t\\\\.\\pipe\\x\t\t\t\t",
        "PICK\topen\tp1\t\t\t..\\up.mp4\t",
        "PICK\topen\tp1\t\t\t\trelative\\start",
        "PICK\topen\tp1\t\t\t\t\\\\.\\PhysicalDrive0",
    ] {
        assert_eq!(None, NavigationRequest::try_parse(line), "{line:?}");
    }
}

/// A start that could not be navigated to is not sent; the pick still is, and opens wherever the
/// app would have.
#[test]
fn formatting_drops_a_start_that_would_not_be_accepted() {
    let request = CommandLineRequest {
        pick: Some(PickRequest::new(PickMode::Open, "p1", "", "", "", r"..\relative").unwrap()),
        ..CommandLineRequest::default()
    };
    let parsed = NavigationRequest::try_parse(&NavigationRequest::format(&request)).unwrap();
    assert_eq!("", parsed.pick.unwrap().start);
}
