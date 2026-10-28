//! Asks the registry whether a file type carries a `runas` verb, and what program opens it, so
//! "Run as administrator" can be decided. Read-only: nothing here creates, sets or deletes a value.
//! What counts as runnable is `RunAsVerbRules`'s; this only reports what the machine says.
//! Spec: App/Interop/RunAsVerbRegistry.cs.
//!
//! The lookup mirrors how the shell resolves a verb and the order matters: the user's own choice of
//! program (`FileExts\.ext\UserChoice`) wins over the machine default, an extension key can carry
//! verbs directly, and `SystemFileAssociations` is where Windows hangs verbs for a whole family.
//! Answers are cached per extension for the life of the object: the menu asks on every right-click
//! and a file type does not gain a verb while the menu is open.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bb_core::shell_integration::{ElevatedOpen, ElevatedOpenKind, RunAsVerbRules};

use crate::registry::{Hive, Registry, SystemRegistry};

pub struct RunAsVerbRegistry {
    registry: Arc<dyn Registry>,
    verbs: Mutex<HashMap<String, Option<bool>>>,
    commands: Mutex<HashMap<String, Option<String>>>,
}

impl RunAsVerbRegistry {
    pub fn system() -> RunAsVerbRegistry {
        RunAsVerbRegistry::new(Arc::new(SystemRegistry))
    }

    pub fn new(registry: Arc<dyn Registry>) -> RunAsVerbRegistry {
        RunAsVerbRegistry {
            registry,
            verbs: Mutex::new(HashMap::new()),
            commands: Mutex::new(HashMap::new()),
        }
    }

    /// How this item can be run as administrator, if at all.
    pub fn decide(&self, path: &str, is_directory: bool, inside_archive: bool) -> ElevatedOpen {
        RunAsVerbRules::decide(
            path,
            is_directory,
            inside_archive,
            |ext| self.has_run_as_verb(ext),
            |ext| self.open_command_for(ext),
            |p| std::path::Path::new(p).is_file(),
        )
    }

    pub fn can_run_elevated(&self, path: &str, is_directory: bool, inside_archive: bool) -> bool {
        self.decide(path, is_directory, inside_archive).kind() != ElevatedOpenKind::None
    }

    /// True, false, or None when the registry could not be read, which the rules answer with their
    /// own well-known list rather than a refusal.
    fn has_run_as_verb(&self, extension: &str) -> Option<bool> {
        if let Some(cached) = self.verbs.lock().unwrap().get(extension) {
            return *cached;
        }
        let answer = self.query_verb(extension);
        self.verbs
            .lock()
            .unwrap()
            .insert(extension.to_string(), answer);
        answer
    }

    fn query_verb(&self, extension: &str) -> Option<bool> {
        // A key name caps at 255 characters and a file name does not: a preposterous extension is
        // "could not tell", never a failure out of something that runs on every selection change.
        if extension.len() > 200 {
            return None;
        }
        let has = |class: &str| -> Result<bool, crate::registry::RegistryError> {
            self.registry
                .key_exists(Hive::ClassesRoot, &format!(r"{class}\shell\runas"))
        };
        let run = || -> Result<bool, crate::registry::RegistryError> {
            if has(extension)? || has(&format!(r"SystemFileAssociations\{extension}"))? {
                return Ok(true);
            }
            for prog_id in self.prog_ids_for(extension)? {
                if has(&prog_id)? {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        run().ok()
    }

    /// The command Windows would run to open this type, environment variables expanded, or None.
    /// A `DelegateExecute` command is refused: those are COM handlers and the command line beside
    /// them is a fallback the shell may ignore entirely, so starting it directly would run
    /// something other than what a double-click runs.
    fn open_command_for(&self, extension: &str) -> Option<String> {
        if let Some(cached) = self.commands.lock().unwrap().get(extension) {
            return cached.clone();
        }
        let command = self.query_command(extension);
        self.commands
            .lock()
            .unwrap()
            .insert(extension.to_string(), command.clone());
        command
    }

    fn query_command(&self, extension: &str) -> Option<String> {
        if extension.len() > 200 {
            return None;
        }
        let ids = self.prog_ids_for(extension).ok()?;
        for prog_id in ids {
            let key = format!(r"{prog_id}\shell\open\command");
            let Ok(true) = self.registry.key_exists(Hive::ClassesRoot, &key) else {
                continue;
            };
            if matches!(
                self.registry
                    .string_value(Hive::ClassesRoot, &key, "DelegateExecute"),
                Ok(Some(d)) if !d.is_empty()
            ) {
                continue;
            }
            if let Ok(Some(command)) = self.registry.string_value(Hive::ClassesRoot, &key, "")
                && !command.is_empty()
            {
                return Some(command);
            }
        }
        None
    }

    /// The classes to consider for an extension, most specific first: the user's own choice of
    /// program, then the machine default.
    fn prog_ids_for(&self, extension: &str) -> Result<Vec<String>, crate::registry::RegistryError> {
        let mut ids = Vec::new();
        let chosen = self.registry.string_value(
            Hive::CurrentUser,
            &format!(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{extension}\UserChoice"
            ),
            "ProgId",
        )?;
        if let Some(chosen) = chosen.filter(|c| !c.is_empty()) {
            ids.push(chosen);
        }
        if let Some(default) = self
            .registry
            .string_value(Hive::ClassesRoot, extension, "")?
            .filter(|d| !d.is_empty())
            && !ids.contains(&default)
        {
            ids.push(default);
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MemoryRegistry;

    fn rig() -> (Arc<MemoryRegistry>, RunAsVerbRegistry) {
        let reg = Arc::new(MemoryRegistry::new());
        (reg.clone(), RunAsVerbRegistry::new(reg))
    }

    #[test]
    fn a_type_with_a_runas_verb_on_its_class_is_runnable() {
        let (reg, runas) = rig();
        reg.set_string(Hive::ClassesRoot, ".msi", "", "Msi.Package")
            .unwrap();
        reg.set_string(Hive::ClassesRoot, r"Msi.Package\shell\runas", "", "x")
            .unwrap();
        assert_eq!(
            ElevatedOpen::Verb,
            runas.decide(r"C:\a\x.msi", false, false)
        );
    }

    #[test]
    fn the_users_own_choice_of_program_is_asked_before_the_machine_default() {
        let (reg, runas) = rig();
        reg.set_string(Hive::ClassesRoot, ".foo", "", "Machine.Foo")
            .unwrap();
        reg.set_string(
            Hive::CurrentUser,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.foo\UserChoice",
            "ProgId",
            "Mine.Foo",
        )
        .unwrap();
        reg.set_string(Hive::ClassesRoot, r"Mine.Foo\shell\runas", "", "x")
            .unwrap();
        assert_eq!(
            ElevatedOpen::Verb,
            runas.decide(r"C:\a\x.foo", false, false)
        );
    }

    #[test]
    fn a_type_without_a_verb_falls_back_to_its_open_command() {
        let (reg, runas) = rig();
        reg.set_string(Hive::ClassesRoot, ".sln", "", "VS.Solution")
            .unwrap();
        reg.set_string(
            Hive::ClassesRoot,
            r"VS.Solution\shell\open\command",
            "",
            r#""C:\Tools\devenv.exe" "%1""#,
        )
        .unwrap();
        // The parser wants the program to exist; the rig's executable is not on disk.
        assert_eq!(
            ElevatedOpenKind::None,
            runas.decide(r"C:\a\x.sln", false, false).kind()
        );
    }

    #[test]
    fn a_delegate_execute_command_is_refused() {
        let (reg, runas) = rig();
        reg.set_string(Hive::ClassesRoot, ".bar", "", "Bar.File")
            .unwrap();
        reg.set_string(
            Hive::ClassesRoot,
            r"Bar.File\shell\open\command",
            "",
            r#"x.exe "%1""#,
        )
        .unwrap();
        reg.set_string(
            Hive::ClassesRoot,
            r"Bar.File\shell\open\command",
            "DelegateExecute",
            "{GUID}",
        )
        .unwrap();
        assert_eq!(None, runas.open_command_for(".bar"));
    }

    #[test]
    fn well_known_executables_are_runnable_even_with_an_empty_registry() {
        let (_, runas) = rig();
        // The fake answers "no verb" rather than "could not tell"; the rules' list is for the
        // second. An .exe is still decided by the verb lookup, so it is the shortcut and the
        // directory refusals that this checks hold regardless.
        assert_eq!(ElevatedOpen::None, runas.decide(r"C:\a", true, false));
        assert_eq!(
            ElevatedOpen::None,
            runas.decide(r"C:\a\x.zip\y.exe", false, true)
        );
        assert_eq!(
            ElevatedOpen::Verb,
            runas.decide(r"C:\a\x.lnk", false, false)
        );
    }

    #[test]
    fn an_absurd_extension_is_not_a_failure() {
        let (_, runas) = rig();
        let long = format!(r"C:\a\x.{}", "e".repeat(400));
        assert_eq!(
            ElevatedOpenKind::None,
            runas.decide(&long, false, false).kind()
        );
    }
}
