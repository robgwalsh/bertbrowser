// Port of tests/BertBrowser.Core.Tests/IndexerBannerRulesTests.cs: the banner replaced an elevation
// prompt that used to appear on every launch, so most of what matters here is when it stays quiet.
use bb_core::mft::{IndexerBanner, IndexerBannerRules, IndexerPresence};

fn decide(
    presence: IndexerPresence,
    can_start: bool,
    dismissed: bool,
    failure: &str,
) -> IndexerBanner {
    IndexerBannerRules::decide(presence, can_start, dismissed, failure)
}

fn default() -> IndexerBanner {
    decide(IndexerPresence::NotRunning, true, false, "")
}

#[test]
fn offers_to_start_a_helper_when_there_is_none_and_one_could_be_started() {
    let banner = default();

    assert!(banner.show);
    assert!(banner.can_start);
    assert_eq!(IndexerBannerRules::OFFER, banner.message);
}

/// It has to say what is missing, what it costs, and that it is not every launch.
#[test]
fn the_offer_explains_why_administrator_rights_are_wanted() {
    let offer = IndexerBannerRules::OFFER.to_lowercase();
    assert!(offer.contains("administrator rights"));
    assert!(offer.contains("once"));
    assert!(offer.contains("search"));
}

#[test]
fn says_nothing_while_a_helper_is_running() {
    assert!(!decide(IndexerPresence::Running, true, false, "").show);
}

/// The in-process indexer and the UI harness have no helper to miss. Reporting one absent would
/// put this strip across every scripted screenshot.
#[test]
fn says_nothing_for_a_host_that_has_no_helper_at_all() {
    assert!(!decide(IndexerPresence::NotApplicable, true, false, "").show);
}

#[test]
fn says_nothing_once_dismissed() {
    assert!(!decide(IndexerPresence::NotRunning, true, true, "").show);
}

/// **The anti-double-messaging rule.** A failure already appears in the status bar with its own
/// retry link; the banner saying it too reads as two problems instead of one.
#[test]
fn leaves_failures_to_the_status_bar() {
    assert!(
        !decide(
            IndexerPresence::NotRunning,
            true,
            false,
            "Search index off — permission declined."
        )
        .show
    );
}

/// A standard user cannot elevate, so this would be an offer that could never be taken up,
/// repeated on every single launch. The status bar says it once instead.
#[test]
fn never_nags_a_user_who_cannot_elevate() {
    assert!(!decide(IndexerPresence::NotRunning, false, false, "").show);
}

#[test]
fn a_banner_and_a_failure_are_never_both_speaking() {
    for presence in IndexerPresence::ALL {
        for can_start in [true, false] {
            let with_failure =
                IndexerBannerRules::decide(presence, can_start, false, "something went wrong");
            assert!(!with_failure.show, "{presence:?} can_start={can_start}");
        }
    }
}

#[test]
fn a_hidden_banner_carries_no_message_to_render() {
    let banner = decide(IndexerPresence::Running, true, false, "");

    assert_eq!("", banner.message);
    assert!(!banner.can_start);
}
