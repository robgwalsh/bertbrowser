//! Making Win+E open BertBrowser instead of File Explorer, described as data so the writer, the
//! remover, the status reader and the tests cannot disagree.
//!
//! **This is a separate door from the folder handler, and an undocumented one.** Win+E never asks
//! `Directory` or `Drive`: it invokes the `opennewwindow` verb of one shell CLSID, which HKLM points
//! back at itself through `DelegateExecute`. A key of the same name under HKCU shadows it. Microsoft
//! documents none of this, so an update can take it away, and the failure is the safe one: Win+E goes
//! back to File Explorer.
//!
//! **What it covers, measured:** Win+E and `explorer.exe` run with no arguments. `explorer.exe <folder>`
//! never comes this way, so anything that names a folder keeps opening File Explorer.
//!
//! **`DelegateExecute = ""` is mandatory**, for the reason in `folder_handler`: a resolving delegate
//! beats the command line outright.
//!
//! The command takes no arguments, which is a bare launch: a normal start when the app is closed, and
//! the running window brought forward when it is not (`SingleInstance` has to let a request with no
//! targets through for that).

use super::ShellIntegrationError;
use super::folder_handler::{
    FolderHandlerRegistration, FolderHandlerRules, FolderHandlerState, ShellRegistryValue,
    same_path,
};

pub struct WinEHandlerRegistration;

impl WinEHandlerRegistration {
    pub const CLSID_KEY: &'static str =
        r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}";
    pub const SHELL_KEY: &'static str =
        r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell";
    pub const VERB_KEY: &'static str =
        r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow";
    pub const COMMAND_KEY: &'static str = r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow\command";

    /// The command line the shell runs: the program alone, quoted because the install path sits
    /// under a profile directory that may contain spaces.
    pub fn command_for(executable_path: &str) -> String {
        format!("\"{executable_path}\"")
    }

    /// Everything the registration writes, for one executable path, in the order it is written.
    /// The command comes first because a key holding only an empty delegate is the broken shape: it
    /// silences the stock delegate with nothing to run in its place.
    pub fn values_for(
        executable_path: &str,
    ) -> Result<Vec<ShellRegistryValue>, ShellIntegrationError> {
        if executable_path.trim().is_empty() {
            return Err(ShellIntegrationError::BlankExecutablePath);
        }
        Ok(vec![
            ShellRegistryValue::new(Self::COMMAND_KEY, "", &Self::command_for(executable_path)),
            ShellRegistryValue::new(
                Self::COMMAND_KEY,
                FolderHandlerRegistration::DELEGATE_EXECUTE,
                "",
            ),
        ])
    }

    /// The keys left empty by a removal, deepest first, which are pruned only if nothing else lives
    /// in them. The command key itself is deleted outright.
    pub fn keys_to_prune() -> [&'static str; 3] {
        [Self::VERB_KEY, Self::SHELL_KEY, Self::CLSID_KEY]
    }
}

/// The raw values a reader found under the HKCU key. Stock Windows has neither.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WinEHandlerReading {
    pub command: Option<String>,
    pub delegate_execute: Option<String>,
}

impl WinEHandlerReading {
    pub fn new(command: Option<&str>, delegate_execute: Option<&str>) -> WinEHandlerReading {
        WinEHandlerReading {
            command: command.map(str::to_string),
            delegate_execute: delegate_execute.map(str::to_string),
        }
    }
}

/// Decides what a [`WinEHandlerReading`] means, in the same four states as the folder handler.
pub struct WinEHandlerRules;

impl WinEHandlerRules {
    /// Anything naming another program, or delegating with no program of ours to overrule, is decided
    /// first and wins, because every caller treats it as "do not touch".
    pub fn classify(reading: &WinEHandlerReading, executable_path: &str) -> FolderHandlerState {
        let delegate = reading.delegate_execute.as_deref().map(str::trim);
        let delegate_is_blank = delegate == Some("");

        let Some(program) = Self::registered_program(reading) else {
            return match delegate {
                None => FolderHandlerState::NotRegistered,
                // The broken shape: reported as ours so the toggle has something to switch off.
                Some("") => FolderHandlerState::RegisteredToThisAppStale,
                Some(_) => FolderHandlerState::RegisteredToAnotherApp,
            };
        };

        if !FolderHandlerRules::is_this_app(Some(&program)) {
            return FolderHandlerState::RegisteredToAnotherApp;
        }

        let intact = same_path(&program, executable_path)
            && FolderHandlerRules::arguments_in(reading.command.as_deref()).is_empty()
            && delegate_is_blank;
        if intact {
            FolderHandlerState::RegisteredToThisApp
        } else {
            FolderHandlerState::RegisteredToThisAppStale
        }
    }

    /// Whether the startup repair should rewrite the registration. As narrow as
    /// [`FolderHandlerRules::should_repair`], for the same reasons: never one that is absent or
    /// another program's, and never a stale path whose executable still exists, which is what stops
    /// a debug build run beside a real install from repointing Win+E at itself.
    pub fn should_repair(
        reading: &WinEHandlerReading,
        executable_path: &str,
        exists: impl Fn(&str) -> bool,
    ) -> bool {
        if Self::classify(reading, executable_path) != FolderHandlerState::RegisteredToThisAppStale
        {
            return false;
        }
        let Some(registered) = Self::registered_program(reading) else {
            return false;
        };
        !exists(&registered) || same_path(&registered, executable_path)
    }

    /// The program the registered command names, or None if there is none.
    pub fn registered_program(reading: &WinEHandlerReading) -> Option<String> {
        FolderHandlerRules::program_in(reading.command.as_deref())
    }
}
