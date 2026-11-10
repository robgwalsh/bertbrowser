//! How the app sits in Windows' shell, decided as data: being the folder handler and Win+E, composing
//! the context menu from the shell's extensions, custom commands, and finding programs to launch.
//! Nothing here opens a registry key or starts a process. Split out of bb-core.

pub mod custom_commands;
pub mod launch;
pub mod shell_integration;
pub mod shellmenu;
