//! Registering this app as the shell's handler for folders and drives, in the current user's hive.
//! What a registration is and when one is stale are `bb_core::shell_integration`'s; this only writes
//! and reads, through the [`Registry`] seam. Spec: App/Interop/FolderHandlerRegistry.cs,
//! App/Services/FolderHandlerService.cs.
//!
//! Nothing here runs unasked except the two safety nets: the uninstall hook (a registration left
//! pointing at a deleted executable would break every folder double-click on the machine) and the
//! startup repair, which never creates a registration that is absent.

use std::sync::Arc;

use bb_core::shell_integration::{
    FolderHandlerReading, FolderHandlerRegistration, FolderHandlerRules, FolderHandlerState,
    ShellRegistryValue,
};

use crate::registry::{Hive, Registry, SystemRegistry};

pub struct FolderHandlerRegistry {
    registry: Arc<dyn Registry>,
    executable: String,
}

impl FolderHandlerRegistry {
    /// Over the machine's registry, naming the app executable beside the one running.
    pub fn system() -> FolderHandlerRegistry {
        FolderHandlerRegistry::new(Arc::new(SystemRegistry), Self::executable_beside_this_one())
    }

    pub fn new(registry: Arc<dyn Registry>, executable: String) -> FolderHandlerRegistry {
        FolderHandlerRegistry {
            registry,
            executable,
        }
    }

    /// `BertBrowser.exe` in the folder of the running executable. The helpers sit beside it, and so
    /// does this: a registration names the product, never whichever binary happened to do the writing.
    pub fn executable_beside_this_one() -> String {
        let folder = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        folder
            .join(FolderHandlerRegistration::EXECUTABLE_NAME)
            .to_string_lossy()
            .into_owned()
    }

    pub fn executable(&self) -> &str {
        &self.executable
    }

    pub fn state(&self) -> FolderHandlerState {
        FolderHandlerRules::classify(&self.read(), &self.executable)
    }

    /// The program registered for folders when it is not this app, for the settings page to name.
    pub fn other_program(&self) -> Option<String> {
        FolderHandlerRules::registered_program(&self.read())
    }

    pub fn set(&self, open_folders_here: bool) -> bool {
        if open_folders_here {
            self.try_register()
        } else {
            self.try_unregister()
        }
    }

    /// The backstop behind the uninstall hook. Narrow on purpose: see `FolderHandlerRules::should_repair`.
    pub fn repair_if_stale(&self) {
        if !FolderHandlerRules::should_repair(&self.read(), &self.executable, |p| {
            std::path::Path::new(p).exists()
        }) {
            return;
        }
        // Removed before writing, so a repair also clears anything the registration should not
        // contain: a masked default-verb guard above all, which re-writing the command alone would
        // leave in place.
        self.try_unregister();
        self.try_register();
    }

    pub fn read(&self) -> FolderHandlerReading {
        let get = |key: &str| self.registry.string_value(Hive::CurrentUser, key, "");
        let read = || -> Result<FolderHandlerReading, crate::registry::RegistryError> {
            let directory_verb = get(FolderHandlerRegistration::DIRECTORY_SHELL_KEY)?;
            let directory_command = get(FolderHandlerRegistration::DIRECTORY_COMMAND_KEY)?;
            let drive_verb = get(FolderHandlerRegistration::DRIVE_SHELL_KEY)?;
            let drive_command = get(FolderHandlerRegistration::DRIVE_COMMAND_KEY)?;
            Ok(FolderHandlerReading::new(
                directory_verb.as_deref(),
                directory_command.as_deref(),
                drive_verb.as_deref(),
                drive_command.as_deref(),
            ))
        };
        read().unwrap_or_else(|_| FolderHandlerReading::none())
    }

    pub fn try_register(&self) -> bool {
        let Ok(values) = FolderHandlerRegistration::values_for(&self.executable) else {
            return false;
        };
        if !self.write(&values) {
            self.try_unregister();
            return false;
        }

        // The verb has to be *there* before anything names it as the default action, and "there"
        // means read back off the registry rather than inferred from the writes having not failed.
        // Naming a verb that is missing is what sends a folder double-click to whatever third-party
        // verb enumerates first.
        let expected = FolderHandlerRegistration::command_for(&self.executable);
        for key in FolderHandlerRegistration::command_keys() {
            let actual = self
                .registry
                .string_value(Hive::CurrentUser, key, "")
                .ok()
                .flatten();
            if actual.as_deref() != Some(expected.as_str()) {
                self.try_unregister();
                return false;
            }
        }

        if !self.write(&FolderHandlerRegistration::guard_values()) {
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

    pub fn try_unregister(&self) -> bool {
        let run = || -> Result<(), crate::registry::RegistryError> {
            let mut removed_something = false;
            for key in FolderHandlerRegistration::keys_to_remove() {
                if !self.registry.key_exists(Hive::CurrentUser, key)? {
                    continue;
                }
                self.registry.delete_tree(Hive::CurrentUser, key)?;
                removed_something = true;
            }

            let to_remove = FolderHandlerRegistration::values_to_remove();
            for value in &to_remove {
                // Only what this app wrote. Anything else on that shared key belongs to whoever put
                // it there, including nothing at all, which is what an untouched machine looks like
                // and must stay looking like.
                let current = self.registry.string_value(
                    Hive::CurrentUser,
                    &value.key_path,
                    &value.value_name,
                )?;
                if current.as_deref() != Some(value.expected_data.as_str()) {
                    continue;
                }
                self.registry.delete_value(
                    Hive::CurrentUser,
                    &value.key_path,
                    &value.value_name,
                )?;
                removed_something = true;
            }

            // Only tidy up after a removal that actually removed something. Unregistering when nothing
            // of ours was there must be a pure no-op: an empty `Drive\shell` is a key Windows or
            // another program may have put there.
            if removed_something {
                for value in &to_remove {
                    if self.registry.is_empty(Hive::CurrentUser, &value.key_path)? == Some(true) {
                        let _ = self.registry.delete_key(Hive::CurrentUser, &value.key_path);
                    }
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

    fn rig() -> (Arc<MemoryRegistry>, FolderHandlerRegistry) {
        let reg = Arc::new(MemoryRegistry::new());
        (
            reg.clone(),
            FolderHandlerRegistry::new(reg, EXE.to_string()),
        )
    }

    #[test]
    fn registering_then_reading_says_registered_to_this_app() {
        let (_, handler) = rig();
        assert!(handler.try_register());
        assert_eq!(FolderHandlerState::RegisteredToThisApp, handler.state());
    }

    #[test]
    fn unregistering_an_untouched_machine_changes_nothing() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DRIVE_SHELL_KEY,
            "",
            "",
        )
        .unwrap();
        let before = reg.dump();
        assert!(handler.try_unregister());
        assert_eq!(
            before,
            reg.dump(),
            "an empty Drive\\shell is not ours to delete"
        );
    }

    #[test]
    fn unregistering_leaves_another_programs_default_verb_alone() {
        let (reg, handler) = rig();
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DIRECTORY_SHELL_KEY,
            "",
            "other",
        )
        .unwrap();
        assert!(handler.try_register());
        // Registering overwrote the default verb with ours; put the other one back, as another
        // program would, and check that removal leaves it.
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DIRECTORY_SHELL_KEY,
            "",
            "other",
        )
        .unwrap();
        assert!(handler.try_unregister());
        assert_eq!(
            Some("other".to_string()),
            reg.string_value(
                Hive::CurrentUser,
                FolderHandlerRegistration::DIRECTORY_SHELL_KEY,
                ""
            )
            .unwrap()
        );
        assert!(
            !reg.key_exists(
                Hive::CurrentUser,
                FolderHandlerRegistration::DIRECTORY_OPEN_KEY
            )
            .unwrap()
        );
    }

    #[test]
    fn unregistering_removes_the_verb_and_prunes_the_emptied_shell_keys() {
        let (reg, handler) = rig();
        assert!(handler.try_register());
        assert!(handler.try_unregister());
        assert_eq!(FolderHandlerState::NotRegistered, handler.state());
        assert!(
            !reg.key_exists(
                Hive::CurrentUser,
                FolderHandlerRegistration::DRIVE_SHELL_KEY
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
    fn a_registration_pointing_at_a_missing_executable_is_repaired_only_when_it_is_this_apps() {
        let (reg, handler) = rig();
        assert!(handler.try_register());
        // Pointing at the same path but incomplete: the open verb's command is damaged.
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DIRECTORY_COMMAND_KEY,
            "",
            &format!("\"{EXE}\" \"%1\""),
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
        for key in [
            FolderHandlerRegistration::DIRECTORY_COMMAND_KEY,
            FolderHandlerRegistration::DRIVE_COMMAND_KEY,
        ] {
            reg.set_string(Hive::CurrentUser, key, "", r#""C:\Other\other.exe" "%1""#)
                .unwrap();
        }
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DIRECTORY_SHELL_KEY,
            "",
            "open",
        )
        .unwrap();
        reg.set_string(
            Hive::CurrentUser,
            FolderHandlerRegistration::DRIVE_SHELL_KEY,
            "",
            "open",
        )
        .unwrap();
        let before = reg.dump();
        handler.repair_if_stale();
        assert_eq!(before, reg.dump());
        assert!(handler.other_program().is_some());
    }
}
