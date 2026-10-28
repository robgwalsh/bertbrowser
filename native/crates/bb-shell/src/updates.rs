//! Keeping an installed copy up to date. Spec: App/Services/UpdateService.cs, Core/Updates/ReleaseChannel.cs.
//!
//! The decision of what to do, in order (check, download, stage for the next exit, offer to restart
//! now), is [`check_and_stage`] over the [`UpdateFeed`] seam, so it is tested against a fake feed
//! and never touches the network. The Velopack feed is the one real implementation. **Updates are
//! mandatory: an update is staged to apply when the process exits, whether or not the user restarts
//! now.** A failed check never takes down the app; the next launch retries.
//!
//! Which feed an installed copy follows is decided from its own version string
//! ([`ReleaseChannel`]): an unstable build ships as a pre-release, so refusing pre-releases would
//! leave it unable to ever see its own feed, and a release must keep refusing them, or the newest
//! thing on offer is the unstable pre-release, which carries no feed for its channel.

use std::any::Any;
use std::sync::Arc;

use bb_core::updates::ReleaseChannel;
use velopack::sources::{AutoSource, GithubSource, NoneSource};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackApp};

pub const REPO_URL: &str = "https://github.com/robgwalsh/bertbrowser";

/// Points the update check at a local `Releases` directory (or any static file host), for
/// end-to-end update testing without touching GitHub.
pub const URL_OVERRIDE_VARIABLE: &str = "BERTBROWSER_UPDATE_URL";

/// An update the feed found. `handle` is the feed's own record of it, handed back to the feed.
#[derive(Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub handle: Arc<dyn Any + Send + Sync>,
}

/// Where updates come from and how they are applied. Blocking: runs on a background thread.
pub trait UpdateFeed: Send + Sync {
    /// Dev builds (not installed by Velopack) have nothing to update.
    fn is_installed(&self) -> bool;
    fn check(&self) -> Result<Option<AvailableUpdate>, String>;
    fn download(&self, update: &AvailableUpdate) -> Result<(), String>;
    /// Applies the update when this process exits, without restarting.
    fn stage_for_exit(&self, update: &AvailableUpdate) -> Result<(), String>;
    /// Exits this process now, applies the update and starts the new version.
    fn apply_and_restart(&self, update: &AvailableUpdate) -> Result<(), String>;
}

/// What a check came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    NotInstalled,
    UpToDate,
    /// Downloaded and staged for the next exit. `restarted` is true when the user chose to restart
    /// now (in which case the process is about to end).
    Staged {
        version: String,
        restarted: bool,
    },
    Failed(String),
}

/// Check, download, stage, and offer a restart. `offer_restart` is shown the new version and says
/// whether to restart now.
pub fn check_and_stage(
    feed: &dyn UpdateFeed,
    offer_restart: &dyn Fn(&str) -> bool,
) -> UpdateOutcome {
    if !feed.is_installed() {
        return UpdateOutcome::NotInstalled;
    }
    let update = match feed.check() {
        Ok(Some(update)) => update,
        Ok(None) => return UpdateOutcome::UpToDate,
        Err(why) => return UpdateOutcome::Failed(why),
    };
    if let Err(why) = feed.download(&update) {
        return UpdateOutcome::Failed(why);
    }
    if let Err(why) = feed.stage_for_exit(&update) {
        return UpdateOutcome::Failed(why);
    }
    let restart = offer_restart(&update.version);
    if restart && let Err(why) = feed.apply_and_restart(&update) {
        return UpdateOutcome::Failed(why);
    }
    UpdateOutcome::Staged {
        version: update.version,
        restarted: restart,
    }
}

/// The sentence the user is told once an update is waiting.
pub fn ready_message(version: &str) -> String {
    format!(
        "BertBrowser {version} has been downloaded and will be installed when you close the app.\n\nRestart now to update immediately?"
    )
}

/// Which GitHub releases to look at: pre-releases too only for an unstable build.
pub fn includes_prereleases(installed_version: &str) -> bool {
    ReleaseChannel::is_unstable(Some(installed_version))
}

/// The Velopack feed. `None` manager means this copy was not installed by Velopack.
pub struct VelopackFeed {
    manager: Option<UpdateManager>,
}

impl VelopackFeed {
    /// For this installed copy, from GitHub (or `override_url` when given).
    pub fn for_this_install(override_url: Option<&str>) -> VelopackFeed {
        // A source-less manager is enough to read the installed version, which picks the source.
        let installed = UpdateManager::new(NoneSource {}, None, None)
            .ok()
            .map(|m| m.get_current_version_as_string());
        let manager = installed.and_then(|version| {
            match override_url.filter(|u| !u.is_empty()) {
                Some(url) => UpdateManager::new(AutoSource::new(url), None, None),
                None => UpdateManager::new(
                    GithubSource::new(REPO_URL, None, includes_prereleases(&version)),
                    None,
                    None,
                ),
            }
            .ok()
        });
        VelopackFeed { manager }
    }

    pub fn from_environment() -> VelopackFeed {
        VelopackFeed::for_this_install(std::env::var(URL_OVERRIDE_VARIABLE).ok().as_deref())
    }

    fn manager(&self) -> Result<&UpdateManager, String> {
        self.manager
            .as_ref()
            .ok_or_else(|| "not installed".to_string())
    }

    fn info(update: &AvailableUpdate) -> Result<&UpdateInfo, String> {
        update
            .handle
            .downcast_ref::<UpdateInfo>()
            .ok_or_else(|| "not an update this feed found".to_string())
    }
}

impl UpdateFeed for VelopackFeed {
    fn is_installed(&self) -> bool {
        self.manager.is_some()
    }

    fn check(&self) -> Result<Option<AvailableUpdate>, String> {
        match self
            .manager()?
            .check_for_updates()
            .map_err(|e| e.to_string())?
        {
            UpdateCheck::UpdateAvailable(info) => Ok(Some(AvailableUpdate {
                version: info.TargetFullRelease.Version.clone(),
                handle: Arc::new(*info),
            })),
            UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => Ok(None),
        }
    }

    fn download(&self, update: &AvailableUpdate) -> Result<(), String> {
        self.manager()?
            .download_updates(Self::info(update)?, None)
            .map_err(|e| e.to_string())
    }

    fn stage_for_exit(&self, update: &AvailableUpdate) -> Result<(), String> {
        self.manager()?
            .wait_exit_then_apply_updates(Self::info(update)?, true, false, Vec::<String>::new())
            .map_err(|e| e.to_string())
    }

    fn apply_and_restart(&self, update: &AvailableUpdate) -> Result<(), String> {
        self.manager()?
            .apply_updates_and_restart(Self::info(update)?)
            .map_err(|e| e.to_string())
    }
}

/// Handles the installer's hooks (`--veloapp-install`, `--veloapp-updated`, `--veloapp-obsolete`,
/// `--veloapp-uninstall`) and applies a pending update at startup. **Must run before anything else:**
/// it exits the process when invoked as a hook, and must not be gated behind an instance check.
/// `before_uninstall` runs inside the uninstall with a 30-second budget and should touch nothing
/// but the registry, since the process exits straight after. Returns normally for an ordinary launch.
pub fn run_installer_hooks(args: Vec<String>, before_uninstall: impl FnOnce() + 'static) {
    // Uninstalling deletes the whole install directory, so a folder-handler registration left behind
    // would point the Windows shell at an executable that is gone, and since it owns the Directory
    // and Drive open verbs, every folder double-click on the machine would fail, with the registry
    // as the only way back.
    VelopackApp::build()
        .set_args(args)
        .on_before_uninstall_fast_callback(move |_| before_uninstall())
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        installed: bool,
        found: Option<&'static str>,
        fail_at: Option<&'static str>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl Fake {
        fn step(&self, name: &'static str) -> Result<(), String> {
            self.calls.lock().unwrap().push(name);
            if self.fail_at == Some(name) {
                Err(format!("{name} failed"))
            } else {
                Ok(())
            }
        }
    }

    impl UpdateFeed for Fake {
        fn is_installed(&self) -> bool {
            self.installed
        }
        fn check(&self) -> Result<Option<AvailableUpdate>, String> {
            self.step("check")?;
            Ok(self.found.map(|v| AvailableUpdate {
                version: v.to_string(),
                handle: Arc::new(()),
            }))
        }
        fn download(&self, _: &AvailableUpdate) -> Result<(), String> {
            self.step("download")
        }
        fn stage_for_exit(&self, _: &AvailableUpdate) -> Result<(), String> {
            self.step("stage")
        }
        fn apply_and_restart(&self, _: &AvailableUpdate) -> Result<(), String> {
            self.step("restart")
        }
    }

    fn installed(found: Option<&'static str>) -> Fake {
        Fake {
            installed: true,
            found,
            ..Fake::default()
        }
    }

    #[test]
    fn a_dev_build_is_never_asked_anything() {
        let feed = Fake::default();
        assert_eq!(
            UpdateOutcome::NotInstalled,
            check_and_stage(&feed, &|_| panic!("no prompt"))
        );
        assert!(feed.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn nothing_new_means_nothing_downloaded() {
        let feed = installed(None);
        assert_eq!(
            UpdateOutcome::UpToDate,
            check_and_stage(&feed, &|_| panic!("no prompt"))
        );
        assert_eq!(vec!["check"], *feed.calls.lock().unwrap());
    }

    #[test]
    fn an_update_is_staged_for_exit_before_the_user_is_asked_anything() {
        let feed = installed(Some("2.1.0"));
        let asked = Mutex::new(Vec::new());
        let outcome = check_and_stage(&feed, &|v| {
            // Staged already: declining the restart must not lose the update.
            assert_eq!(
                vec!["check", "download", "stage"],
                *feed.calls.lock().unwrap()
            );
            asked.lock().unwrap().push(v.to_string());
            false
        });
        assert_eq!(
            UpdateOutcome::Staged {
                version: "2.1.0".into(),
                restarted: false
            },
            outcome
        );
        assert_eq!(vec!["2.1.0".to_string()], *asked.lock().unwrap());
        assert!(!feed.calls.lock().unwrap().contains(&"restart"));
    }

    #[test]
    fn accepting_the_offer_restarts() {
        let feed = installed(Some("2.1.0"));
        let outcome = check_and_stage(&feed, &|_| true);
        assert_eq!(
            UpdateOutcome::Staged {
                version: "2.1.0".into(),
                restarted: true
            },
            outcome
        );
        assert_eq!(Some(&"restart"), feed.calls.lock().unwrap().last());
    }

    #[test]
    fn a_failure_at_any_step_is_reported_and_stops_there() {
        for step in ["check", "download", "stage", "restart"] {
            let feed = Fake {
                fail_at: Some(step),
                ..installed(Some("2.1.0"))
            };
            let outcome = check_and_stage(&feed, &|_| true);
            assert_eq!(
                UpdateOutcome::Failed(format!("{step} failed")),
                outcome,
                "{step}"
            );
            assert_eq!(Some(&step), feed.calls.lock().unwrap().last(), "{step}");
        }
    }

    #[test]
    fn only_an_unstable_build_looks_at_pre_releases() {
        assert!(includes_prereleases("2.0.0-unstable.42"));
        assert!(includes_prereleases("2.0.0-UNSTABLE.1"));
        assert!(!includes_prereleases("2.0.0"));
        assert!(!includes_prereleases("2.0.0-beta.1"));
        assert!(!includes_prereleases("2.0.0+unstable-notes"));
    }

    #[test]
    fn the_ready_message_names_the_version_and_the_restart() {
        let m = ready_message("2.1.0");
        assert!(m.contains("2.1.0") && m.contains("Restart now"));
    }

    #[test]
    fn a_copy_that_velopack_did_not_install_has_no_manager() {
        // Run from `target\`, which no installer put there.
        let feed = VelopackFeed::for_this_install(None);
        assert!(!feed.is_installed());
        assert_eq!(
            UpdateOutcome::NotInstalled,
            check_and_stage(&feed, &|_| panic!("no prompt"))
        );
    }
}
