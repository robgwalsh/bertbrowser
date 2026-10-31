//! Registering this app as what Win+E opens, in the current user's hive. What a registration is and
//! when one is stale are `bb_core::shell_integration`'s; this only writes and reads, through the
//! [`Registry`] seam.
//!
//! As with the folder handler, nothing here runs unasked except the two safety nets: the uninstall
//! hook (a registration left pointing at a deleted executable would make Win+E fail) and the startup
//! repair, which never creates a registration that is absent.

use std::sync::Arc;

use bb_core::shell_integration::{
    FolderHandlerRegistration, FolderHandlerState, ShellRegistryValue, WinEHandlerReading,
    WinEHandlerRegistration, WinEHandlerRules,
};

use crate::folder_handler::FolderHandlerRegistry;
use crate::registry::{Hive, Registry, RegistryError, SystemRegistry};

pub struct WinEHandlerRegistry {
    registry: Arc<dyn Registry>,
    executable: String,
}

impl WinEHandlerRegistry {
    /// Over the machine's registry, naming the app executable beside the one running.
    pub fn system() -> WinEHandlerRegistry {
        WinEHandlerRegistry::new(
            Arc::new(SystemRegistry),
            FolderHandlerRegistry::executable_beside_this_one(),
        )
    }

    pub fn new(registry: Arc<dyn Registry>, executable: String) -> WinEHandlerRegistry {
        WinEHandlerRegistry {
            registry,
            executable,
        }
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    pub fn state(&self) -> FolderHandlerState {
        WinEHandlerRules::classify(&self.read(), &self.executable)
    }

    /// The program registered for Win+E, for the settings page to name when it is not this app.
    pub fn other_program(&self) -> Option<String> {
        WinEHandlerRules::registered_program(&self.read())
    }

    pub fn set(&self, open_here: bool) -> bool {
        if open_here {
            self.try_register()
        } else {
            self.try_unregister()
        }
    }

    /// The backstop behind the uninstall hook. Narrow on purpose: see `WinEHandlerRules::should_repair`.
    pub fn repair_if_stale(&self) {
        if !WinEHandlerRules::should_repair(&self.read(), &self.executable, |p| {
            std::path::Path::new(p).exists()
        }) {
            return;
        }
        self.try_unregister();
        self.try_register();
    }

    pub fn read(&self) -> WinEHandlerReading {
        let get = |name: &str| {
            self.registry.string_value(
                Hive::CurrentUser,
                WinEHandlerRegistration::COMMAND_KEY,
                name,
            )
        };
        let read = || -> Result<WinEHandlerReading, RegistryError> {
            let command = get("")?;
            let delegate = get(FolderHandlerRegistration::DELEGATE_EXECUTE)?;
            Ok(WinEHandlerReading::new(
                command.as_deref(),
                delegate.as_deref(),
            ))
        };
        read().unwrap_or_default()
    }

    pub fn try_register(&self) -> bool {
        let Ok(values) = WinEHandlerRegistration::values_for(&self.executable) else {
            return false;
        };
        if !self.write(&values) {
            self.try_unregister();
            return false;
        }
        // Read back rather than inferred from the writes having not failed.
        if self.state() != FolderHandlerState::RegisteredToThisApp {
            self.try_unregister();
            return false;
        }
        true
    }

    fn write(&self, values: &[ShellRegistryValue]) -> bool {
        values.iter().all(|v| {
            self.registry
                .set_string(Hive::CurrentUser, &v.key_path, &v.value_name, &v.data)
                .is_ok()
        })
    }

    /// Removes only what this app wrote. Another program's registration is theirs, and a machine with
    /// nothing of ours on it stays exactly as it was.
    pub fn try_unregister(&self) -> bool {
        if matches!(
            self.state(),
            FolderHandlerState::NotRegistered | FolderHandlerState::RegisteredToAnotherApp
        ) {
            return true;
        }
        let run = || -> Result<(), RegistryError> {
            self.registry
                .delete_tree(Hive::CurrentUser, WinEHandlerRegistration::COMMAND_KEY)?;
            for key in WinEHandlerRegistration::keys_to_prune() {
                if self.registry.is_empty(Hive::CurrentUser, key)? == Some(true) {
                    self.registry.delete_key(Hive::CurrentUser, key)?;
                }
            }
            Ok(())
        };
        run().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MemoryRegistry;

    const EXE: &str = r"C:\Apps\BertBrowser\BertBrowser.exe";
    const FOREIGN: &str = r#""C:\Other\other.exe""#;

    fn rig() -> (Arc<MemoryRegistry>, WinEHandlerRegistry) {
        let reg = Arc::new(MemoryRegistry::new());
        (reg.clone(), WinEHandlerRegistry::new(reg, EXE.to_string()))
    }

    #[test]
    fn registering_then_reading_says_registered_to_this_app() {
        let (reg, handler) = rig();
        assert!(handler.set(true));
        assert_eq!(FolderHandlerState::RegisteredToThisApp, handler.state());
        assert_eq!(
            Some(format!("\"{EXE}\"")),
            reg.string_value(Hive::CurrentUser, WinEHandlerRegistration::COMMAND_KEY, "")
                .unwrap()
        );
    }

    #[test]
    fn unregistering_removes_every_key_it_created() {
        let (reg, handler) = rig();
        assert!(handler.set(true));
        assert!(handler.set(false));
        assert_eq!(FolderHandlerState::NotRegistered, handler.state());
        assert!(
            !reg.key_exists(Hive::CurrentUser, WinEHandlerRegistration::CLSID_KEY)
                .unwrap()
        );
    }

    #[test]
    fn unregistering_an_untouched_machine_changes_nothing() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            WinEHandlerRegistration::SHELL_KEY,
            "",
            "",
        )
        .unwrap();
        let before = reg.dump();
        assert!(handler.try_unregister());
        assert_eq!(before, reg.dump());
    }

    #[test]
    fn unregistering_leaves_another_programs_registration_alone() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            WinEHandlerRegistration::COMMAND_KEY,
            "",
            FOREIGN,
        )
        .unwrap();
        let before = reg.dump();
        assert!(handler.try_unregister());
        assert_eq!(before, reg.dump());
        assert_eq!(FolderHandlerState::RegisteredToAnotherApp, handler.state());
        assert!(handler.other_program().is_some());
    }

    #[test]
    fn unregistering_keeps_whatever_else_lives_beside_it() {
        let (reg, handler) = rig();
        let sibling = format!(r"{}\other", WinEHandlerRegistration::SHELL_KEY);
        reg.set_string(Hive::CurrentUser, &sibling, "", "someone else")
            .unwrap();
        let on_the_verb = "MUIVerb";
        assert!(handler.set(true));
        reg.set_string(
            Hive::CurrentUser,
            WinEHandlerRegistration::VERB_KEY,
            on_the_verb,
            "someone else",
        )
        .unwrap();
        assert!(handler.set(false));
        assert!(
            !reg.key_exists(Hive::CurrentUser, WinEHandlerRegistration::COMMAND_KEY)
                .unwrap()
        );
        assert_eq!(
            Some("someone else".to_string()),
            reg.string_value(Hive::CurrentUser, &sibling, "").unwrap()
        );
        assert_eq!(
            Some("someone else".to_string()),
            reg.string_value(
                Hive::CurrentUser,
                WinEHandlerRegistration::VERB_KEY,
                on_the_verb
            )
            .unwrap()
        );
    }

    #[test]
    fn a_repair_never_creates_a_registration_that_is_absent() {
        let (reg, handler) = rig();
        handler.repair_if_stale();
        assert!(reg.dump().is_empty());
    }

    #[test]
    fn a_damaged_registration_of_this_app_is_repaired() {
        let (reg, handler) = rig();
        assert!(handler.set(true));
        reg.delete_value(
            Hive::CurrentUser,
            WinEHandlerRegistration::COMMAND_KEY,
            "DelegateExecute",
        )
        .unwrap();
        assert_eq!(
            FolderHandlerState::RegisteredToThisAppStale,
            handler.state()
        );
        handler.repair_if_stale();
        assert_eq!(FolderHandlerState::RegisteredToThisApp, handler.state());
    }

    #[test]
    fn another_programs_registration_is_never_repaired() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            WinEHandlerRegistration::COMMAND_KEY,
            "",
            FOREIGN,
        )
        .unwrap();
        let before = reg.dump();
        handler.repair_if_stale();
        assert_eq!(before, reg.dump());
    }

    #[test]
    fn turning_it_on_takes_over_from_another_program_when_asked() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            WinEHandlerRegistration::COMMAND_KEY,
            "",
            FOREIGN,
        )
        .unwrap();
        assert!(handler.set(true));
        assert_eq!(FolderHandlerState::RegisteredToThisApp, handler.state());
    }
}
