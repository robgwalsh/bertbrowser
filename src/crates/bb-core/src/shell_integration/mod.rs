//! What it takes to be the shell's folder handler, and what "run as administrator" can mean for a
//! file — all of it decided as data, so the registry reader/writer in the app and the tests cannot
//! disagree. Nothing here opens a registry key. Spec: Core/Services/ShellIntegration/*.

mod folder_handler;
mod run_as_verb_rules;
mod shell_open_command;
mod win_e_handler;

pub use folder_handler::{
    FolderHandlerReading, FolderHandlerRegistration, FolderHandlerRules, FolderHandlerState,
    ShellRegistryValue, ShellRegistryValueRef,
};
pub use run_as_verb_rules::{ElevatedOpen, ElevatedOpenKind, RunAsVerbRules};
pub use shell_open_command::{ShellOpenCommand, ShellOpenCommandParser};
pub use win_e_handler::{WinEHandlerReading, WinEHandlerRegistration, WinEHandlerRules};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellIntegrationError {
    /// A registration cannot name nothing.
    BlankExecutablePath,
}

impl std::fmt::Display for ShellIntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellIntegrationError::BlankExecutablePath => {
                write!(f, "the executable path must be non-empty")
            }
        }
    }
}

impl std::error::Error for ShellIntegrationError {}
