//! Turns the `code` launcher on `PATH` into the editor's own executable.
//!
//! `code` is not a program. It is `…\Microsoft VS Code\bin\code.cmd`, a batch file that starts
//! `Code.exe` as a Node process to talk to the running instance — so starting it through the shell
//! starts cmd.exe, and a console window sits beside the editor for as long as that takes. Nothing
//! about the window is fixable at the launch (hiding it means not shell-executing, which is what
//! makes file associations and the `runas` verb work), so the shim is stepped over instead.
//!
//! Pure, and takes its probe as an argument. It refuses far more than it accepts — anything that is
//! not a `.cmd`/`.bat` in a `bin` folder with a known editor beside it comes back None, and the
//! caller keeps the launcher it already had. Spec: Core/Services/VSCodePath.cs.

use super::win_path::{combine, directory_name, extension};
use bb_paths::file_name;

pub struct VsCodePath;

impl VsCodePath {
    /// What an install root may call the editor, best first. Insiders and VSCodium are here because
    /// their launchers are the ones a stem-matching rule would miss: the shim is
    /// `code-insiders.cmd` and the program beside it is `Code - Insiders.exe`.
    pub const EXECUTABLES: [&'static str; 4] = [
        "Code.exe",
        "Code - Insiders.exe",
        "VSCodium.exe",
        "VSCodium - Insiders.exe",
    ];

    /// The editor executable a resolved launcher stands in for, or None when it is not a shim of
    /// that shape or nothing beside it is an editor. `launcher` is an absolute path, as
    /// `ExecutablePath::resolve` returns; `resolve` probes for a file, answering with its absolute
    /// path or None — `ExecutablePath::resolve` over a fully qualified path is exactly that.
    pub fn behind_launcher(
        launcher: Option<&str>,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Option<String> {
        let launcher = launcher?;
        if launcher.trim().is_empty() {
            return None;
        }

        let ext = extension(launcher);
        if !ext.eq_ignore_ascii_case(".cmd") && !ext.eq_ignore_ascii_case(".bat") {
            return None;
        }

        // The folder has to be a `bin`, and the editor is the level above it. Without that the rule
        // would be "run the .exe next to any .cmd", which is a different program than the one the
        // user asked for whenever the guess is wrong.
        let bin = directory_name(launcher)?;
        if !file_name(&bin).eq_ignore_ascii_case("bin") {
            return None;
        }

        let root = directory_name(&bin)?;
        if root.is_empty() {
            return None;
        }

        Self::EXECUTABLES
            .iter()
            .find_map(|name| resolve(&combine(&root, name)))
    }
}
