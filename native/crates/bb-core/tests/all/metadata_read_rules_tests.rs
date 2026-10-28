// Port of tests/BertBrowser.Core.Tests/MetadataReadRulesTests.cs.
use bb_core::attributes::{
    ARCHIVE, DIRECTORY, HIDDEN, NORMAL, OFFLINE, READONLY, RECALL_ON_DATA_ACCESS, RECALL_ON_OPEN,
    REPARSE_POINT,
};
use bb_core::columns::MetadataReadRules;
use bb_core::preview::PreviewClassifier;

#[test]
fn an_ordinary_file_is_read() {
    for attributes in [NORMAL, ARCHIVE, HIDDEN, READONLY] {
        assert!(MetadataReadRules::may_read(attributes), "{attributes:#x}");
    }
}

/// The rule this class exists for. Reading a placeholder makes the sync provider fetch it, so a
/// Dimensions column scrolled down a synced photo folder would download the whole folder a row at a
/// time. Drop this and the tests below go green while the app quietly empties someone's OneDrive
/// quota onto their disk.
#[test]
fn a_cloud_placeholder_is_never_opened() {
    assert!(!MetadataReadRules::may_read(NORMAL | OFFLINE));
}

#[test]
fn the_two_recall_bits_dot_net_does_not_name_are_refused_too() {
    // Not in .NET's FileAttributes at all, which is why the classifier names them and why this
    // rule asks that class rather than writing the mask out a second time.
    assert!(!MetadataReadRules::may_read(NORMAL | RECALL_ON_OPEN));
    assert!(!MetadataReadRules::may_read(NORMAL | RECALL_ON_DATA_ACCESS));
}

#[test]
fn a_reparse_point_is_refused_rather_than_followed() {
    assert!(!MetadataReadRules::may_read(NORMAL | REPARSE_POINT));
}

#[test]
fn a_directory_is_skipped() {
    assert!(!MetadataReadRules::may_read(DIRECTORY));
}

#[test]
fn the_predicate_is_shared_with_the_preview_pane_rather_than_reimplemented() {
    // Said in that class's own words, so re-writing the mask here goes red.
    assert!(PreviewClassifier::is_cloud_placeholder(OFFLINE));
}
