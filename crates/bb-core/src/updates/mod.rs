//! Which update feed an installed copy follows, decided from nothing but its own version string.
//! Spec: Core/Updates/ReleaseChannel.cs.

pub struct ReleaseChannel;

impl ReleaseChannel {
    pub const UNSTABLE: &'static str = "unstable";

    /// True when the pre-release tag's first identifier is `unstable`, ignoring case.
    pub fn is_unstable(version: Option<&str>) -> bool {
        let Some(value) = version.map(str::trim).filter(|v| !v.is_empty()) else {
            return false;
        };
        // Build metadata sits after '+' and is not part of the pre-release tag. Stripping it first
        // is what keeps "1.2.3+unstable-notes" a stable version.
        let value = value.split_once('+').map_or(value, |(head, _)| head);
        let Some((_, prerelease)) = value.split_once('-') else {
            return false;
        };
        // The tag is dot-separated identifiers and the channel is the first of them, so
        // "unstable.42" matches while "beta.1" does not.
        let channel = prerelease.split('.').next().unwrap_or("");
        channel.eq_ignore_ascii_case(Self::UNSTABLE)
    }
}
