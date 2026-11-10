//! When to offer to start the index helper, and when to say nothing. The app used to raise an
//! elevation prompt on every launch; this replaced it with a strip that explains what is missing
//! and offers to fetch it. The interesting cases are the ones where it must *not* appear: a banner
//! nobody can act on, or one repeating what the status bar already says, is worse than the prompt
//! it replaced. Pure, so every row of that table is a test. Spec:
//! Core/Services/Mft/IndexerBannerRules.cs.

use super::presence::IndexerPresence;

/// What, if anything, the window should say about a missing index helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexerBanner {
    /// Whether the banner appears at all.
    pub show: bool,
    /// What it says. Empty when it does not appear.
    pub message: String,
    /// Whether to offer the button that raises the elevation prompt.
    pub can_start: bool,
}

impl IndexerBanner {
    pub fn hidden() -> IndexerBanner {
        IndexerBanner {
            show: false,
            message: String::new(),
            can_start: false,
        }
    }
}

pub struct IndexerBannerRules;

impl IndexerBannerRules {
    /// What the ordinary "no helper yet" banner says. It answers three questions at once: what is
    /// missing, what it costs, and how often it will cost it.
    pub const OFFER: &'static str = "Whole-PC search and folder sizes need the index helper. \
It asks for administrator rights once and keeps running until you sign out.";

    /// `can_start`: whether asking for a helper could actually produce one; false for an account
    /// that cannot elevate, and for a second copy of the app that will never own the endpoint.
    pub fn decide(
        presence: IndexerPresence,
        can_start: bool,
        dismissed: bool,
        failure: &str,
    ) -> IndexerBanner {
        // Asked to go away for this session.
        if dismissed {
            return IndexerBanner::hidden();
        }
        // This host has no separate helper (the in-process indexer, or the UI harness). Saying one
        // is missing would put the banner over every scripted screenshot.
        if presence == IndexerPresence::NotApplicable {
            return IndexerBanner::hidden();
        }
        if presence == IndexerPresence::Running {
            return IndexerBanner::hidden();
        }
        // Something went wrong and the status bar is already saying so, with its own retry link.
        // Two places describing one problem reads as two problems.
        if !failure.is_empty() {
            return IndexerBanner::hidden();
        }
        // Nothing to offer: a standard user cannot elevate, and a banner they can do nothing about
        // on every launch is nagging. The status bar says it once, quietly.
        if !can_start {
            return IndexerBanner::hidden();
        }
        IndexerBanner {
            show: true,
            message: Self::OFFER.to_string(),
            can_start: true,
        }
    }
}
