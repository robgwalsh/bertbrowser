//! Making the Windows shell open a folder or a drive in BertBrowser instead of Explorer, described
//! as data so the writer, the remover, the status reader and the tests cannot disagree.
//!
//! **Windows has no "default file manager" setting.** Explorer's own `Capabilities` key registers
//! no file or folder associations at all, so nothing in Default Apps can express "open folders in
//! this program"; overriding the shell verb is the only mechanism there is.
//!
//! **The scope is `Directory` and `Drive`, deliberately never `Folder`.** Folders and drives inherit
//! their open verb from the `Folder` class, whose command is Explorer behind a `DelegateExecute`
//! CLSID. Writing *more specific* verbs on `Directory` and `Drive` takes the filesystem cases and
//! leaves `Folder` alone — This PC, Control Panel, zip browsing and Explorer's own navigation keep
//! working even if this registration is left pointing at nothing.
//!
//! **The `(Default)` on the `shell` key has to be written, and writing it is the most dangerous
//! thing here.** Both `Directory\shell` and `Drive\shell` ship with `"none"`; measured, with that in
//! place the shell never consults a verb at all. Naming a verb is what makes the shell invoke one —
//! and naming one that is *missing* makes the shell fall through to the first verb it enumerates,
//! which is whatever a third party installed. An early version wrote this value first, and on a
//! machine with a NordVPN entry under `Directory\shell`, double-clicking a folder opened NordVPN.
//! Hence [`guard_values`](FolderHandlerRegistration::guard_values) is separate from
//! [`values_for`](FolderHandlerRegistration::values_for) and is written last, only after the
//! command has been read back, and rolled back with everything else if any write fails.
//!
//! **`DelegateExecute = ""` is mandatory.** `Directory` and `Drive` derive from `Folder`, and the
//! verb they inherit carries `CLSID_ExecuteFolder`; a resolving `DelegateExecute` beats the command
//! line outright, so without the empty shadow every folder went to Explorer while the keys looked
//! exactly right.
//!
//! Removal is asymmetric with writing: `Directory\shell` is a key other installers put their own
//! verbs under, so unregistering deletes the `open` subtree and the one `(Default)` value this app
//! wrote, never the `shell` key itself. Spec: FolderHandlerRegistration.cs, FolderHandlerState.cs.

use bb_paths::{PathKey, file_name};

use super::ShellIntegrationError;

/// One value the registration writes. `value_name` is `""` for a key's default value, which is
/// what the shell reads for both a verb name and a command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRegistryValue {
    pub key_path: String,
    pub value_name: String,
    pub data: String,
}

impl ShellRegistryValue {
    fn new(key_path: &str, value_name: &str, data: &str) -> ShellRegistryValue {
        ShellRegistryValue {
            key_path: key_path.to_string(),
            value_name: value_name.to_string(),
            data: data.to_string(),
        }
    }
}

/// One value the registration removes, and the data it must still hold to be removable. The
/// expected data is the safety catch: these values sit on keys shared with other programs, so
/// removal deletes only what this app actually wrote — a default verb someone else has since
/// pointed elsewhere is theirs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRegistryValueRef {
    pub key_path: String,
    pub value_name: String,
    pub expected_data: String,
}

pub struct FolderHandlerRegistration;

impl FolderHandlerRegistration {
    /// The program a registration must name to be recognised as this app's.
    pub const EXECUTABLE_NAME: &'static str = "BertBrowser.exe";

    /// The verb, and the name of the subkey holding it.
    pub const OPEN_VERB: &'static str = "open";

    /// Matches what `Folder\shell\open` already carries. `Document` means the shell invokes the
    /// verb once per selected item rather than once for the whole selection, which is what the
    /// single-instance hand-off is shaped for.
    pub const MULTI_SELECT_MODEL: &'static str = "Document";

    /// Written **empty** on each command key, and the registration does nothing without it (see
    /// the module notes).
    pub const DELEGATE_EXECUTE: &'static str = "DelegateExecute";

    pub const DIRECTORY_SHELL_KEY: &'static str = r"Software\Classes\Directory\shell";
    pub const DRIVE_SHELL_KEY: &'static str = r"Software\Classes\Drive\shell";
    pub const DIRECTORY_OPEN_KEY: &'static str = r"Software\Classes\Directory\shell\open";
    pub const DRIVE_OPEN_KEY: &'static str = r"Software\Classes\Drive\shell\open";
    pub const DIRECTORY_COMMAND_KEY: &'static str =
        r"Software\Classes\Directory\shell\open\command";
    pub const DRIVE_COMMAND_KEY: &'static str = r"Software\Classes\Drive\shell\open\command";

    /// Opening from the shell is always a **new tab**. Double-clicking a folder in Explorer gets
    /// you a window showing that folder; it does not retarget a window you were already using.
    /// The flag goes on the registration rather than into the request so that `bertbrowser <path>`
    /// typed at a prompt keeps its documented meaning.
    pub const NEW_TAB_FLAG: &'static str = "--new-tab";

    /// Everything the command line carries after the program. `%1` is the folder or drive path,
    /// quoted because folder names routinely contain spaces. Named separately so the rules can tell
    /// a registration written by an older build — right program, outdated arguments — from a
    /// current one, and let the startup repair bring it up to date.
    pub const ARGUMENT_TAIL: &'static str = "--new-tab \"%1\"";

    /// The command line the shell runs. The program is quoted for the same reason the argument is
    /// — the install path sits under a profile directory that may contain spaces.
    pub fn command_for(executable_path: &str) -> String {
        format!("\"{executable_path}\" {}", Self::ARGUMENT_TAIL)
    }

    /// Everything the registration writes, for one executable path, **in the order it must be
    /// written**. The command comes first, and that ordering is load-bearing: creating the `open`
    /// key registers a verb; setting its command is what makes the verb usable. A write
    /// interrupted between the two leaves a verb the shell can select and cannot run, so the
    /// deepest key — which brings its parent into existence with it — is written first and the
    /// decoration after.
    pub fn values_for(
        executable_path: &str,
    ) -> Result<Vec<ShellRegistryValue>, ShellIntegrationError> {
        if executable_path.trim().is_empty() {
            return Err(ShellIntegrationError::BlankExecutablePath);
        }

        let command = Self::command_for(executable_path);
        Ok(vec![
            ShellRegistryValue::new(Self::DIRECTORY_COMMAND_KEY, "", &command),
            ShellRegistryValue::new(Self::DIRECTORY_COMMAND_KEY, Self::DELEGATE_EXECUTE, ""),
            ShellRegistryValue::new(
                Self::DIRECTORY_OPEN_KEY,
                "MultiSelectModel",
                Self::MULTI_SELECT_MODEL,
            ),
            ShellRegistryValue::new(Self::DRIVE_COMMAND_KEY, "", &command),
            ShellRegistryValue::new(Self::DRIVE_COMMAND_KEY, Self::DELEGATE_EXECUTE, ""),
            ShellRegistryValue::new(
                Self::DRIVE_OPEN_KEY,
                "MultiSelectModel",
                Self::MULTI_SELECT_MODEL,
            ),
        ])
    }

    /// The default-verb values, kept apart from [`values_for`](Self::values_for) because they must
    /// be written **last and only once the verb they name has been read back from the registry**.
    /// Naming a verb that is not there hands the double-click to whatever third-party verb
    /// enumerates first.
    pub fn guard_values() -> Vec<ShellRegistryValue> {
        vec![
            ShellRegistryValue::new(Self::DIRECTORY_SHELL_KEY, "", Self::OPEN_VERB),
            ShellRegistryValue::new(Self::DRIVE_SHELL_KEY, "", Self::OPEN_VERB),
        ]
    }

    /// The command keys whose default value must read back correctly before the guard values may
    /// be written.
    pub fn command_keys() -> [&'static str; 2] {
        [Self::DIRECTORY_COMMAND_KEY, Self::DRIVE_COMMAND_KEY]
    }

    /// Keys deleted whole when unregistering — this app's verb and everything under it.
    pub fn keys_to_remove() -> [&'static str; 2] {
        [Self::DIRECTORY_OPEN_KEY, Self::DRIVE_OPEN_KEY]
    }

    /// Values deleted when unregistering, on keys that must survive because other programs keep
    /// their own verbs under them.
    pub fn values_to_remove() -> Vec<ShellRegistryValueRef> {
        [Self::DIRECTORY_SHELL_KEY, Self::DRIVE_SHELL_KEY]
            .into_iter()
            .map(|key| ShellRegistryValueRef {
                key_path: key.to_string(),
                value_name: String::new(),
                expected_data: Self::OPEN_VERB.to_string(),
            })
            .collect()
    }
}

/// What the registry currently says about who opens folders and drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderHandlerState {
    /// Nothing of ours is there; the shell inherits Explorer from the `Folder` class.
    NotRegistered,
    /// Registered, complete, and naming the executable that is asking.
    RegisteredToThisApp,
    /// Ours, but wrong — a stale executable path, a half-written pair, or the default verb no
    /// longer pointing at it. This is the only state the startup self-heal may repair.
    RegisteredToThisAppStale,
    /// Some other program owns the verb. Never overwritten without being asked.
    RegisteredToAnotherApp,
}

/// The raw values a reader found. Nothing has been interpreted yet, which is the point: it is what
/// lets the interpretation be tested without opening a registry key. A default verb is None when
/// HKCU has no value of its own — in which case the HKLM `"none"` shows through and no verb is the
/// default action.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FolderHandlerReading {
    pub directory_default_verb: Option<String>,
    pub directory_command: Option<String>,
    pub drive_default_verb: Option<String>,
    pub drive_command: Option<String>,
}

impl FolderHandlerReading {
    pub fn new(
        directory_default_verb: Option<&str>,
        directory_command: Option<&str>,
        drive_default_verb: Option<&str>,
        drive_command: Option<&str>,
    ) -> FolderHandlerReading {
        FolderHandlerReading {
            directory_default_verb: directory_default_verb.map(str::to_string),
            directory_command: directory_command.map(str::to_string),
            drive_default_verb: drive_default_verb.map(str::to_string),
            drive_command: drive_command.map(str::to_string),
        }
    }

    /// Nothing found at all.
    pub fn none() -> FolderHandlerReading {
        FolderHandlerReading::default()
    }
}

/// Decides what a [`FolderHandlerReading`] means. Pure, so the self-heal rule, the settings
/// toggle's checked state and the "another program owns this" message all read from one
/// classification a test can hold still.
pub struct FolderHandlerRules;

impl FolderHandlerRules {
    /// Classifies a reading against the executable that would be registered. The order of the
    /// checks is the design: `RegisteredToAnotherApp` is decided first and wins outright, because
    /// every caller treats it as "do not touch" — a partial reading where someone else holds one of
    /// the two verbs must not be repaired into ours. Everything after that is ours, and the only
    /// question left is whether it is intact.
    pub fn classify(reading: &FolderHandlerReading, executable_path: &str) -> FolderHandlerState {
        let directory = Self::program_in(reading.directory_command.as_deref());
        let drive = Self::program_in(reading.drive_command.as_deref());

        // Both shell keys must name the verb as their default action, or the shell never invokes it
        // — with the stock "none" in place it uses its own folder navigation instead and a perfect
        // command sits there unread.
        let directory_verb = is_open_verb(reading.directory_default_verb.as_deref());
        let drive_verb = is_open_verb(reading.drive_default_verb.as_deref());

        // The dangerous shape, and the reason this is not folded into `intact` below: a default verb
        // named with no command behind it is what sends a folder double-click to a third party.
        // Reported as ours-and-broken so the toggle shows something to switch off, and switching it
        // off clears the value.
        if directory.is_none() && drive.is_none() {
            return if directory_verb || drive_verb {
                FolderHandlerState::RegisteredToThisAppStale
            } else {
                FolderHandlerState::NotRegistered
            };
        }

        if is_foreign(directory.as_deref()) || is_foreign(drive.as_deref()) {
            return FolderHandlerState::RegisteredToAnotherApp;
        }

        let intact = matches!((&directory, &drive), (Some(d), Some(v))
                if same_path(d, executable_path) && same_path(v, executable_path))
            && current_arguments(reading.directory_command.as_deref())
            && current_arguments(reading.drive_command.as_deref())
            && directory_verb
            && drive_verb;

        if intact {
            FolderHandlerState::RegisteredToThisApp
        } else {
            FolderHandlerState::RegisteredToThisAppStale
        }
    }

    /// Whether the startup repair should rewrite the registration.
    ///
    /// The failure this exists for is the one that matters: a registration naming an executable
    /// that is no longer there makes **every folder double-click fail**, with no way back except
    /// the registry. It is deliberately narrow. A registration that is *absent* is never created —
    /// the user may have removed it outside this app. One belonging to another program is never
    /// touched. And a stale path whose executable still exists is left alone too, which is what
    /// stops a debug build run beside a real install from quietly repointing the shell at
    /// `bin\Debug`: the only stale registrations repaired are ones pointing at something gone, or
    /// already pointing here and merely incomplete.
    pub fn should_repair(
        reading: &FolderHandlerReading,
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

    /// The program a registered command names, or None if nothing is registered. Used for the
    /// message naming another owner, and for asking whether a stale registration points at an
    /// executable that is still on disk.
    pub fn registered_program(reading: &FolderHandlerReading) -> Option<String> {
        Self::program_in(reading.directory_command.as_deref())
            .or_else(|| Self::program_in(reading.drive_command.as_deref()))
    }

    /// The program token of a shell command line: the quoted run if there is one, otherwise
    /// everything up to the first space. None for a blank command or an unterminated quote.
    ///
    /// Not a general command-line parser and does not need to be. It reads back only what
    /// [`FolderHandlerRegistration::command_for`] writes, plus whatever another file manager
    /// wrote — and for that one, all that is wanted is a name to show and the knowledge that it is
    /// not ours.
    pub fn program_in(command: Option<&str>) -> Option<String> {
        let text = command?.trim();
        if text.is_empty() {
            return None;
        }

        if let Some(rest) = text.strip_prefix('"') {
            let end = rest.find('"')?;
            return (end > 0).then(|| rest[..end].to_string());
        }

        let program = text.split(' ').next().unwrap_or("");
        (!program.is_empty()).then(|| program.to_string())
    }

    /// Whether a program token names this app, by file name — the path may be stale, and telling
    /// "ours, moved" from "somebody else's" is exactly what the caller needs.
    pub fn is_this_app(program: Option<&str>) -> bool {
        program.is_some_and(|p| {
            file_name(p).eq_ignore_ascii_case(FolderHandlerRegistration::EXECUTABLE_NAME)
        })
    }

    /// Everything after the program token, or "" when there is nothing.
    pub fn arguments_in(command: Option<&str>) -> String {
        let Some(text) = command.map(str::trim) else {
            return String::new();
        };
        if text.is_empty() {
            return String::new();
        }

        let after = match text.strip_prefix('"') {
            Some(rest) => rest.find('"').map(|i| i + 2),
            None => text.find(' '),
        };
        match after {
            Some(a) if a > 0 && a < text.len() => text[a..].trim().to_string(),
            _ => String::new(),
        }
    }
}

fn is_foreign(program: Option<&str>) -> bool {
    program.is_some() && !FolderHandlerRules::is_this_app(program)
}

/// Whether a registered command carries the arguments this version writes. A build that changed
/// them — adding `--new-tab`, say — leaves behind a registration that still launches the app and no
/// longer does what it now means, which nothing would notice if only the program were compared.
fn current_arguments(command: Option<&str>) -> bool {
    FolderHandlerRules::arguments_in(command)
        .eq_ignore_ascii_case(FolderHandlerRegistration::ARGUMENT_TAIL)
}

fn is_open_verb(verb: Option<&str>) -> bool {
    verb.is_some_and(|v| {
        v.trim()
            .eq_ignore_ascii_case(FolderHandlerRegistration::OPEN_VERB)
    })
}

/// Path comparison through `PathKey` rather than string equality, so a registration differing only
/// in casing is recognised as already correct and the self-heal does not rewrite it on every
/// launch.
fn same_path(a: &str, b: &str) -> bool {
    match (PathKey::canonicalize(a), PathKey::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
