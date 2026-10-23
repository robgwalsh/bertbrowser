// Port of tests/BertBrowser.Core.Tests/CommandTemplateTests.cs.
use bb_core::launch::CommandTemplate;

const SAMPLE_PATH: &str = r"C:\data\docs\report.txt";

#[test]
fn blank_template_defaults_to_quoted_path() {
    assert_eq!(
        "\"C:\\data\\docs\\report.txt\"",
        CommandTemplate::expand(Some(""), SAMPLE_PATH)
    );
}

#[test]
fn null_template_defaults_to_quoted_path() {
    assert_eq!(
        "\"C:\\data\\docs\\report.txt\"",
        CommandTemplate::expand(None, SAMPLE_PATH)
    );
}

#[test]
fn expands_all_tokens() {
    assert_eq!(
        r#""C:\data\docs\report.txt" report.txt C:\data\docs"#,
        CommandTemplate::expand(Some("\"{path}\" {name} {dir}"), SAMPLE_PATH)
    );
}

#[test]
fn tokens_are_case_insensitive() {
    assert_eq!(
        SAMPLE_PATH,
        CommandTemplate::expand(Some("{PATH}"), SAMPLE_PATH)
    );
}

#[test]
fn template_without_tokens_is_returned_verbatim() {
    assert_eq!(
        "--verbose",
        CommandTemplate::expand(Some("--verbose"), SAMPLE_PATH)
    );
}
