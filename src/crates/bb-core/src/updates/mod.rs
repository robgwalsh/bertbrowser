//! Which update feed an installed copy follows, decided from nothing but its own version string.
//! Spec: Core/Updates/ReleaseChannel.cs.

pub struct ReleaseChannel;

impl ReleaseChannel {
    pub const UNSTABLE: &'static str = "unstable";
    /// A checkout's own build, installed by `install.ps1 -Local`. It never takes an update: the
    /// newest published release is not newer than what the developer is working on.
    pub const LOCAL: &'static str = "local";

    /// True when the pre-release tag's first identifier is `unstable`, ignoring case.
    pub fn is_unstable(version: Option<&str>) -> bool {
        Self::channel(version).is_some_and(|c| c.eq_ignore_ascii_case(Self::UNSTABLE))
    }

    /// True when the pre-release tag's first identifier is `local`, ignoring case.
    pub fn is_local(version: Option<&str>) -> bool {
        Self::channel(version).is_some_and(|c| c.eq_ignore_ascii_case(Self::LOCAL))
    }

    /// The first identifier of the pre-release tag, if there is one.
    fn channel(version: Option<&str>) -> Option<&str> {
        let value = version.map(str::trim).filter(|v| !v.is_empty())?;
        // Build metadata sits after '+' and is not part of the pre-release tag. Stripping it first
        // is what keeps "1.2.3+unstable-notes" a stable version.
        let value = value.split_once('+').map_or(value, |(head, _)| head);
        let (_, prerelease) = value.split_once('-')?;
        // The tag is dot-separated identifiers and the channel is the first of them, so
        // "unstable.42" matches while "beta.1" does not.
        prerelease.split('.').next()
    }
}

/// Where the running copy came from, as the title-bar icon's tip says it. Decided from the version
/// Velopack installed, which is `None` for a build running from `out\` (nothing installed it) - not
/// from the build's own version, which a local build does not stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallSource {
    Development,
    Local,
    Unstable,
    Release,
}

impl InstallSource {
    pub fn of(installed_version: Option<&str>) -> InstallSource {
        match installed_version.map(str::trim).filter(|v| !v.is_empty()) {
            None => InstallSource::Development,
            Some(v) if ReleaseChannel::is_local(Some(v)) => InstallSource::Local,
            Some(v) if ReleaseChannel::is_unstable(Some(v)) => InstallSource::Unstable,
            Some(_) => InstallSource::Release,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            InstallSource::Development => "Development build, not installed",
            InstallSource::Local => "Local build, installed from a checkout",
            InstallSource::Unstable => "Unstable build, installed from GitHub",
            InstallSource::Release => "Release, installed from GitHub",
        }
    }

    /// Whether the version alone cannot tell one build from the next, so the tip adds when it was
    /// built and where it runs from: every unstamped build reports the workspace's version.
    pub fn is_unpublished(self) -> bool {
        matches!(self, InstallSource::Development | InstallSource::Local)
    }
}
