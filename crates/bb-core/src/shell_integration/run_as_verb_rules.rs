//! Whether "Run as administrator" can mean anything for a given file.
//!
//! `runas` is a verb registered per file type, not a thing you can do to any file. `exefile` has
//! one; `txtfile` does not, and nothing will ever give it one. Asking the shell to `runas` a `.txt`
//! comes back as `ERROR_NO_ASSOCIATION` — a baffling thing to read about a file that opens fine on
//! a double-click, and until this existed it was the only feedback the app gave.
//!
//! Explorer answers this by greying the item out on anything that is not a program. This goes one
//! step further — see [`RunAsVerbRules::decide`]: where there is no verb but the file has a handler,
//! the *handler* is started elevated with the file as its argument, which is what "run this as
//! administrator" means for a `.sln` or a config file.
//!
//! **This decides and the app reads the registry**, so what counts as runnable is testable without
//! a registry. The registry is asked rather than an extension list consulted, because a hard-coded
//! list is wrong for every file type an installed program registers a `runas` verb for; the list
//! below is only the fallback for when the registry cannot be read at all.
//! Spec: Core/Services/ShellIntegration/RunAsVerbRules.cs.

use bb_paths::file_name;

use super::ShellOpenCommandParser;

/// Which of the two ways of elevating applies, if either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevatedOpenKind {
    /// Nothing to do: no verb, and no handler worth guessing at.
    None,
    /// The shell has a `runas` verb for this type; hand it the file and let it work.
    Verb,
    /// No verb, but the file's handler can be started elevated with the file as its argument.
    Handler,
}

/// How to run an item as administrator, if at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElevatedOpen {
    None,
    Verb,
    /// The program to start, and its arguments with the file already substituted in.
    Handler {
        executable: String,
        arguments: String,
    },
}

impl ElevatedOpen {
    pub fn handler(executable: &str, arguments: &str) -> ElevatedOpen {
        ElevatedOpen::Handler {
            executable: executable.to_string(),
            arguments: arguments.to_string(),
        }
    }

    pub fn kind(&self) -> ElevatedOpenKind {
        match self {
            ElevatedOpen::None => ElevatedOpenKind::None,
            ElevatedOpen::Verb => ElevatedOpenKind::Verb,
            ElevatedOpen::Handler { .. } => ElevatedOpenKind::Handler,
        }
    }
}

pub struct RunAsVerbRules;

impl RunAsVerbRules {
    /// The types measured to carry a `runas` verb, used only when the registry could not be read at
    /// all. Short, and checked rather than guessed: `.com`, `.msi`, `.ps1`, `.vbs` and `.scr` all
    /// look like they belong here and none of them does — `comfile`, `Msi.Package` and the rest
    /// carry no `runas` key, so Windows refuses them exactly as it refuses a text file.
    pub const WELL_KNOWN: [&'static str; 4] = [".exe", ".bat", ".cmd", ".msc"];

    /// A shortcut, which the registry cannot answer for. **The one place the registry is not the
    /// authority.** A `.lnk` carries no verbs of its own — the shell resolves it and applies the
    /// *target's* — and that is not expressed as a `lnkfile\shell\runas` key. Measured both ways:
    /// `runas` on a shortcut to notepad.exe starts an elevated Notepad, and on a shortcut to a
    /// `.txt` it comes back as `ERROR_NO_ASSOCIATION` — the friendly message, not a silence. So it
    /// is offered and the shell decides, which is what Explorer does too.
    const SHORTCUT: &'static str = ".lnk";

    /// The extension to ask about, lower-cased and with its dot, or empty when there is none. A
    /// dotfile is a name, not an extension: `.gitignore` has no type. An extension-less file has no
    /// class either, so neither can carry a verb.
    pub fn extension_of(path: &str) -> String {
        if path.trim().is_empty() {
            return String::new();
        }

        let name = file_name(path.trim_end_matches(['\\', '/']));
        match name.rfind('.') {
            Some(dot) if dot > 0 && dot < name.len() - 1 => name[dot..].to_lowercase(),
            _ => String::new(),
        }
    }

    /// Whether the item may be run as administrator at all. `probe` asks the registry whether an
    /// extension's class carries a `runas` verb; None means it could not tell, which falls back to
    /// [`WELL_KNOWN`](Self::WELL_KNOWN) rather than to a refusal — being unable to read the
    /// registry must not take the feature away from an `.exe`.
    pub fn can_run_elevated(
        path: &str,
        is_directory: bool,
        inside_archive: bool,
        probe: impl Fn(&str) -> Option<bool>,
    ) -> bool {
        Self::decide(
            path,
            is_directory,
            inside_archive,
            probe,
            |_| None,
            |_| false,
        )
        .kind()
            != ElevatedOpenKind::None
    }

    /// How to run this item as administrator, if at all.
    ///
    /// Two ways, and the second is what makes the feature worth having. A handful of types carry a
    /// `runas` verb and the shell knows what to do with them. Everything else — a `.sln`, a
    /// `.docx`, a config file — has no such verb, but it does have an *open command*, and starting
    /// that program elevated with the file as its argument is exactly what "run this as
    /// administrator" means for a document. This goes further than Explorer, on purpose; what it
    /// will not do is guess: a handler it cannot resolve to a real program, or one that never says
    /// where the file goes, leaves the item greyed rather than starting something approximate with
    /// a token. `open_command` is the registered open command for an extension, environment
    /// variables already expanded.
    pub fn decide(
        path: &str,
        is_directory: bool,
        inside_archive: bool,
        has_verb: impl Fn(&str) -> Option<bool>,
        open_command: impl Fn(&str) -> Option<String>,
        exists: impl Fn(&str) -> bool,
    ) -> ElevatedOpen {
        // A folder has no program to run, and nothing inside a container has a path another process
        // could be pointed at — it has to be extracted first, which is what Open already says.
        if is_directory || inside_archive {
            return ElevatedOpen::None;
        }

        let extension = Self::extension_of(path);
        if extension.is_empty() {
            return ElevatedOpen::None;
        }
        if extension == Self::SHORTCUT {
            return ElevatedOpen::Verb;
        }

        let verb =
            has_verb(&extension).unwrap_or_else(|| Self::WELL_KNOWN.contains(&extension.as_str()));
        if verb {
            return ElevatedOpen::Verb;
        }

        match ShellOpenCommandParser::parse(open_command(&extension).as_deref(), path, exists) {
            Some(handler) => ElevatedOpen::Handler {
                executable: handler.executable,
                arguments: handler.arguments,
            },
            None => ElevatedOpen::None,
        }
    }

    /// What to say when it was asked for anyway — through a keyboard shortcut, or a custom command
    /// with the elevated box ticked. Windows' own wording for this is `ERROR_NO_ASSOCIATION`, which
    /// describes the mechanism rather than the problem.
    pub fn cannot_run_message(name: &str) -> String {
        format!("There is no program Windows can run as administrator for '{name}'.")
    }
}
