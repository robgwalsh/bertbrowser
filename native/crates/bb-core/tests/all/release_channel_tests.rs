// Port of tests/BertBrowser.Core.Tests/ReleaseChannelTests.cs.
//
// The channel predicate decides which update feed an installed copy looks at. Make is_unstable
// answer unconditionally, either way, and one half of this goes red.
use bb_core::updates::ReleaseChannel;

#[test]
fn an_unstable_tag_is_the_unstable_channel() {
    for version in [
        "1.1.3-unstable.42",
        "1.1.3-unstable.7+abc1234", // source-linked build metadata
        "1.1.3-unstable",           // no run number
        "1.1.3-UNSTABLE.1",         // the tag is not case-sensitive to us
        "  1.1.3-unstable.42  ",
    ] {
        assert!(ReleaseChannel::is_unstable(Some(version)), "{version:?}");
    }
}

#[test]
fn anything_else_is_stable() {
    for version in [
        Some("1.1.2"),
        Some("1.1.2+abc1234"),
        Some("1.2.0-beta.1"),
        Some("1.2.0-rc.1"),
        Some(""),
        Some("   "),
        None,
    ] {
        assert!(!ReleaseChannel::is_unstable(version), "{version:?}");
    }
}

/// Build metadata is not a pre-release tag, and a version whose metadata merely mentions the word
/// is a stable build.
#[test]
fn the_word_elsewhere_in_the_version_does_not_switch_channel() {
    for version in [
        "1.2.3+unstable",
        "1.2.3+unstable-notes",
        "1.2.3-beta.unstable",
    ] {
        assert!(!ReleaseChannel::is_unstable(Some(version)), "{version:?}");
    }
}
