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

/// A local build never takes an update, so this predicate going wrong either way is a developer's
/// build overwritten by the last release, or a release that never updates again.
#[test]
fn a_local_tag_is_the_local_channel_and_nothing_else_is() {
    for version in [
        "2.0.2-local.20261111093000",
        "2.0.2-LOCAL",
        " 2.0.2-local.1+abc ",
    ] {
        assert!(ReleaseChannel::is_local(Some(version)), "{version:?}");
        assert!(!ReleaseChannel::is_unstable(Some(version)), "{version:?}");
    }
    for version in [
        Some("2.0.1"),
        Some("2.0.2-unstable.88"),
        Some("2.0.2+local"),
        Some("2.0.2-beta.local"),
        Some("2.0.2-localish.1"),
        Some(""),
        None,
    ] {
        assert!(!ReleaseChannel::is_local(version), "{version:?}");
    }
}

/// What the title-bar icon's tip says a copy is. Read off the *installed* version, so a build with
/// nothing installed is a development build whatever version it reports.
#[test]
fn the_install_source_follows_the_installed_version() {
    use bb_core::updates::InstallSource;
    assert_eq!(InstallSource::Development, InstallSource::of(None));
    assert_eq!(InstallSource::Development, InstallSource::of(Some("  ")));
    assert_eq!(
        InstallSource::Local,
        InstallSource::of(Some("2.0.2-local.20261111093000"))
    );
    assert_eq!(
        InstallSource::Unstable,
        InstallSource::of(Some("2.0.2-unstable.88"))
    );
    assert_eq!(InstallSource::Release, InstallSource::of(Some("2.0.1")));
    assert!(InstallSource::Development.is_unpublished());
    assert!(InstallSource::Local.is_unpublished());
    assert!(!InstallSource::Unstable.is_unpublished());
    assert!(!InstallSource::Release.is_unpublished());
}
