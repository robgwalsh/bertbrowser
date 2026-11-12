//! Finding another per-user app that was installed the way this one is (Velopack): where its exe
//! is, or that it is not there at all.
//!
//! Two places, in this order. The default location, `%LOCALAPPDATA%\<pack id>\current\<exe>`, is
//! where nearly every copy is and costs one probe. The installer's own `InstallLocation` under the
//! user's Uninstall key is asked second, because `winget install --location` can put the app
//! somewhere else and that value is the only thing that says where.
//!
//! `current\<exe>` rather than the stub beside it: the stub is Velopack's launcher, which applies a
//! pending update first and then starts the app without waiting, so a file handed to it arrives
//! late or (during an update) not at all.
//!
//! Pure: the two roots and the probe are arguments. Nothing here starts anything.

use super::win_path::is_fully_qualified;

pub struct InstalledApp;

impl InstalledApp {
    /// The key, under the current user's hive, whose subkey `<pack id>` holds `InstallLocation`.
    pub const UNINSTALL_KEY: &'static str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    /// The value of that subkey naming the install directory.
    pub const INSTALL_LOCATION: &'static str = "InstallLocation";

    /// The exe of the app packed as `pack_id`, or None when it is not installed.
    ///
    /// `local_app_data` is `%LOCALAPPDATA%`; `install_location` what the Uninstall key says, if it
    /// says anything. A root that is not a full path is ignored: a relative one would resolve
    /// against wherever this process happened to be started, and that must not pick a program.
    pub fn locate(
        pack_id: &str,
        exe: &str,
        local_app_data: Option<&str>,
        install_location: Option<&str>,
        exists: impl Fn(&str) -> bool,
    ) -> Option<String> {
        if !is_plain_name(pack_id) || !is_plain_name(exe) {
            return None;
        }
        let by_default = local_app_data
            .and_then(root)
            .map(|r| format!("{r}\\{pack_id}\\current\\{exe}"));
        let by_registry = install_location
            .and_then(root)
            .map(|r| format!("{r}\\current\\{exe}"));
        [by_default, by_registry]
            .into_iter()
            .flatten()
            .find(|candidate| exists(candidate))
    }
}

/// A usable root with no trailing separator, or None.
fn root(raw: &str) -> Option<&str> {
    let trimmed = raw.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    (!trimmed.is_empty() && is_fully_qualified(trimmed)).then_some(trimmed)
}

/// One path segment and nothing that could climb out of it.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['\\', '/', ':', '"', '*', '?', '<', '>', '|'])
}
