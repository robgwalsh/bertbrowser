//! The registered open command a static verb runs. The parser lives with ShellIntegration (its C#
//! home, Core/Services/ShellIntegration/ShellOpenCommand.cs); the static verb shares that one
//! implementation rather than carrying a second copy of the quoting rules.

pub use crate::shell_integration::ShellOpenCommand;
