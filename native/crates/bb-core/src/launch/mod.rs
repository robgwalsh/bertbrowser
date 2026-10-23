//! Finding programs to launch, and the argument template of a custom command. Nothing here starts
//! a process: the app has one launcher, and these only decide *what* it would be handed.
//! Spec: Core/Services/CommandTemplate.cs, ExecutablePath.cs, VSCodePath.cs.

mod command_template;
mod executable_path;
mod vscode_path;
pub(crate) mod win_path;

pub use command_template::CommandTemplate;
pub use executable_path::ExecutablePath;
pub use vscode_path::VsCodePath;
