// Port of tests/BertBrowser.Core.Tests/IndexerArgumentsTests.cs: the elevated helper's command
// line. Nothing on it is trusted: it arrives from another process, and the helper has an
// administrator token to lose.
use bb_core::ipc::{IndexerArguments, IndexerCommand};

fn parse(args: &[&str]) -> IndexerArguments {
    IndexerArguments::try_parse(args).unwrap_or_else(|error| panic!("{args:?}: {error}"))
}

fn rejects(args: &[&str]) -> String {
    let error = IndexerArguments::try_parse(args).expect_err("should be rejected");
    assert_ne!("", error, "{args:?}");
    error
}

#[test]
fn parses_the_ordinary_command_line() {
    let options = parse(&["--data-dir", r"C:\Users\Rob\.bertbrowser"]);

    assert_eq!(IndexerCommand::Run, options.command);
    assert_eq!(r"C:\Users\Rob\.bertbrowser", options.data_directory);
}

#[test]
fn parses_the_auto_start_commands() {
    for (flag, expected) in [
        ("--register-autostart", IndexerCommand::RegisterAutoStart),
        (
            "--unregister-autostart",
            IndexerCommand::UnregisterAutoStart,
        ),
    ] {
        let options = parse(&[flag, "--data-dir", r"C:\Data"]);

        assert_eq!(expected, options.command, "{flag}");
        assert_eq!(r"C:\Data", options.data_directory, "{flag}");
    }
}

#[test]
fn argument_order_does_not_matter() {
    let options = parse(&["--data-dir", r"C:\Data", "--register-autostart"]);

    assert_eq!(IndexerCommand::RegisterAutoStart, options.command);
    assert_eq!(r"C:\Data", options.data_directory);
}

#[test]
fn refuses_to_both_register_and_unregister() {
    rejects(&[
        "--register-autostart",
        "--unregister-autostart",
        "--data-dir",
        r"C:\Data",
    ]);
}

#[test]
fn requires_the_data_directory() {
    for args in [&[][..], &["--register-autostart"][..]] {
        rejects(args);
    }
}

#[test]
fn rejects_a_flag_with_no_value() {
    rejects(&["--data-dir"]);
}

/// **The pipe name and the parent process id are gone, not ignored.** The helper derives its own
/// endpoint and has no parent to watch, so there is nothing for a caller to choose, and an
/// argument that parsed but did nothing would read as a check still being made.
#[test]
fn rejects_the_arguments_it_no_longer_takes() {
    for (flag, value) in [("--pipe", "BertBrowser.Index.x"), ("--parent-pid", "4242")] {
        let error = rejects(&["--data-dir", r"C:\Data", flag, value]);
        assert!(error.contains(flag), "{flag}: {error}");
    }
}

/// An unrecognised option is an error, never a positional value, the same rule the user-facing
/// command line follows. A mistyped flag silently becoming a data directory is much worse than a
/// message.
#[test]
fn rejects_an_unrecognised_argument() {
    for extra in ["--elevate", "-p", r"C:\Windows"] {
        rejects(&["--data-dir", r"C:\Data", extra]);
    }
}

#[test]
fn rejects_a_data_directory_that_is_not_an_acceptable_absolute_path() {
    for dir in [r"..\..\Windows", r".bertbrowser", r"\\.\C:", r"C:\Data\*"] {
        rejects(&["--data-dir", dir]);
    }
}

#[test]
fn accepts_a_unc_data_directory() {
    let options = parse(&["--data-dir", r"\\server\share\data"]);

    assert_eq!(r"\\server\share\data", options.data_directory);
}
