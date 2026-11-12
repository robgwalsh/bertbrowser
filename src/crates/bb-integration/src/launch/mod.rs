//! Finding programs to launch, and the argument template of a custom command. Nothing here starts
//! a process: the app has one launcher, and these only decide *what* it would be handed.
//! Spec: Core/Services/CommandTemplate.cs, ExecutablePath.cs, VSCodePath.cs.

mod command_template;
mod executable_path;
mod installed_app;
mod vscode_path;
/// Public for the integration tests (`tests/all/launch/win_path_tests.rs`), not as an API.
#[doc(hidden)]
pub mod win_path;

pub use command_template::{CommandTemplate, escape_trailing_backslashes};
pub use executable_path::ExecutablePath;
pub use installed_app::InstalledApp;
pub use vscode_path::VsCodePath;
