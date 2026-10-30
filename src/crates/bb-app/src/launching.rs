//! Starting other programs for the user: Run as administrator, Open in Terminal, Open in VS Code and
//! the user's own commands. What is started is decided here, from the selection, `PATH` and the
//! command template; **starting it is the launcher seam's and nobody else's** (the app has one
//! `Process.Start`'s worth of launcher, behind `ProcessLauncher`).
//! Spec: ShellViewModel.OpenInTerminal / OpenInVSCode / RunCustomCommand, DirectoryTabView.LaunchTarget.
//!
//! Resolved, not attempted: a launch through the shell reports nothing back, so "is it installed?"
//! has to be answered before anything is handed over. Resolving also decides *which* program runs
//! rather than leaving a bare name to be resolved against whatever folder is current.

use std::sync::Arc;

use bb_core::custom_commands::CustomCommandDefinition;
use bb_core::launch::{CommandTemplate, VsCodePath};
use bb_shell::LaunchRequest;

use crate::app::{Inner, lock};
use crate::ids::{AppError, AppResult};

/// The ids a host hands to `launch_command`.
pub fn is_launch_command(id: &str) -> bool {
    matches!(
        id,
        "file.run-as-admin" | "file.open-terminal" | "file.open-vscode"
    ) || id.starts_with("custom:")
}

/// One thing a command acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub path: String,
    pub is_directory: bool,
}

impl Target {
    fn folder(&self) -> String {
        if self.is_directory {
            self.path.clone()
        } else {
            bb_core::rename::paths::directory_name(&self.path).unwrap_or_default()
        }
    }
}

/// A terminal rooted at the item's folder: Windows Terminal, else PowerShell.
pub(crate) fn terminal_request(
    target: &Target,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Result<LaunchRequest, String> {
    let dir = target.folder();
    if dir.is_empty() {
        return Err("There is no folder here to open.".to_string());
    }
    if let Some(terminal) = resolve("wt.exe") {
        return Ok(LaunchRequest {
            file: terminal,
            arguments: format!("-d \"{dir}\""),
            ..LaunchRequest::default()
        });
    }
    match resolve("powershell.exe") {
        Some(powershell) => Ok(LaunchRequest {
            file: powershell,
            working_directory: dir,
            ..LaunchRequest::default()
        }),
        None => Err(
            "Cannot open terminal: neither Windows Terminal nor PowerShell was found.".to_string(),
        ),
    }
}

/// VS Code on the item. The editor behind the `code` launcher first, then the standard user and
/// system install locations, and only then the launcher itself: `code` is a batch file, and
/// starting one through the shell puts a console window on screen beside the editor it opened. It
/// stays on the list because a console window is better than no editor.
pub(crate) fn vscode_request(
    target: &Target,
    resolve: &dyn Fn(&str) -> Option<String>,
    local_app_data: Option<&str>,
    program_files: Option<&str>,
) -> Result<LaunchRequest, String> {
    let launcher = resolve("code");
    let under = |root: Option<&str>, tail: &str| {
        root.and_then(|r| resolve(&format!("{}\\{tail}", r.trim_end_matches('\\'))))
    };
    let candidates = [
        VsCodePath::behind_launcher(launcher.as_deref(), resolve),
        under(local_app_data, r"Programs\Microsoft VS Code\Code.exe"),
        under(program_files, r"Microsoft VS Code\Code.exe"),
        launcher,
    ];
    let Some(exe) = candidates.into_iter().flatten().next() else {
        return Err("VS Code not found. Install it, or add 'code' to your PATH.".to_string());
    };
    Ok(LaunchRequest {
        file: exe,
        arguments: format!("\"{}\"", target.path),
        ..LaunchRequest::default()
    })
}

/// The user's command, once per target it applies to. `Ok(vec![])` when it applies to none of them.
pub(crate) fn custom_requests(
    command: &CustomCommandDefinition,
    targets: &[Target],
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<LaunchRequest>, String> {
    let matched: Vec<&Target> = targets
        .iter()
        .filter(|t| {
            if t.is_directory {
                command.applies_to_directories
            } else {
                command.applies_to_files
            }
        })
        .collect();
    if matched.is_empty() {
        return Ok(Vec::new());
    }
    // Resolved once, up front: a program that is not there is one message, not one per selected item.
    let Some(program) = resolve(&command.command) else {
        return Err(format!(
            "'{}' failed: '{}' was not found.",
            command.name, command.command
        ));
    };
    Ok(matched
        .into_iter()
        .map(|t| LaunchRequest {
            file: program.clone(),
            arguments: CommandTemplate::expand(Some(&command.arguments), &t.path),
            working_directory: t.folder(),
            elevated: command.run_elevated,
        })
        .collect())
}

impl Inner {
    /// The active tab's selection in list order, or its folder when nothing is selected and
    /// `fall_back_to_folder`.
    fn launch_targets(&self) -> Vec<Target> {
        let Some(tab) = lock(&self.shell).active_tab_id() else {
            return Vec::new();
        };
        let Ok(cell) = self.tab(tab) else {
            return Vec::new();
        };
        let t = lock(&cell);
        let selected: Vec<Target> = t
            .listing
            .rows()
            .iter()
            .filter(|r| t.selection.contains(&r.id))
            .map(|r| Target {
                path: r.full_path.clone(),
                is_directory: r.is_directory,
            })
            .collect();
        if !selected.is_empty() {
            return selected;
        }
        if t.path.is_empty() {
            Vec::new()
        } else {
            vec![Target {
                path: t.path.clone(),
                is_directory: true,
            }]
        }
    }

    fn start(&self, request: &LaunchRequest) -> Result<(), String> {
        self.seams.launcher.start(request)
    }

    /// Runs one of the commands the host cannot run itself because it needs the selection. The
    /// outcome is the status line's; an `Err` is only for an id that is not one of these.
    pub(crate) fn run_launch_command(self: &Arc<Self>, id: &str) -> AppResult<()> {
        if !is_launch_command(id) {
            return Err(AppError::Invalid(format!("'{id}' is not a launch command")));
        }
        if let Some(reason) = self.why_not_now(id)? {
            return Err(AppError::Invalid(reason));
        }
        let targets = self.launch_targets();
        self.launch_on(id, &targets);
        Ok(())
    }

    /// As `run_launch_command`, on one folder rather than the selection: the folder tree's menu,
    /// whose subject is the folder right-clicked, not anything the file list has selected.
    pub(crate) fn run_launch_command_at(self: &Arc<Self>, id: &str, folder: &str) -> AppResult<()> {
        if !is_launch_command(id) || id == "file.run-as-admin" {
            return Err(AppError::Invalid(format!("'{id}' cannot run on a folder")));
        }
        let targets = [Target {
            path: folder.to_string(),
            is_directory: true,
        }];
        self.launch_on(id, &targets);
        Ok(())
    }

    fn launch_on(self: &Arc<Self>, id: &str, targets: &[Target]) {
        let resolve = |p: &str| self.seams.launcher.resolve(p);

        let outcome: Result<String, String> = match id {
            "file.run-as-admin" => match targets.first() {
                None => Err("Select one item.".to_string()),
                Some(t) => {
                    let request = LaunchRequest {
                        file: t.path.clone(),
                        elevated: true,
                        ..LaunchRequest::default()
                    };
                    self.start(&request)
                        .map(|()| format!("Started {}", bb_paths::file_name(&t.path)))
                }
            },
            "file.open-terminal" => match targets.first() {
                None => Err("There is no folder here to open.".to_string()),
                Some(t) => terminal_request(t, &resolve)
                    .and_then(|r| self.start(&r))
                    .map(|()| String::new()),
            },
            "file.open-vscode" => match targets.first() {
                None => Err("There is no folder here to open.".to_string()),
                Some(t) => vscode_request(
                    t,
                    &resolve,
                    std::env::var("LOCALAPPDATA").ok().as_deref(),
                    std::env::var("ProgramFiles").ok().as_deref(),
                )
                .and_then(|r| self.start(&r))
                .map(|()| String::new()),
            },
            custom => match self.custom_command(custom) {
                None => Err(format!("'{custom}' is not a command.")),
                Some(command) => match custom_requests(&command, targets, &resolve) {
                    Err(why) => Err(why),
                    Ok(requests) if requests.is_empty() => Ok(String::new()),
                    Ok(requests) => {
                        let mut failed = None;
                        for request in &requests {
                            if let Err(why) = self.start(request) {
                                failed = Some(format!("'{}' failed: {why}", command.name));
                                break;
                            }
                        }
                        match failed {
                            Some(why) => Err(why),
                            None => Ok(format!(
                                "Ran '{}' on {} item(s)",
                                command.name,
                                requests.len()
                            )),
                        }
                    }
                },
            },
        };
        match outcome {
            Ok(text) if text.is_empty() => {}
            Ok(text) => self.set_active_status(text),
            Err(why) => self.set_active_status(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(map: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            map.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.to_string())
        }
    }

    fn dir(p: &str) -> Target {
        Target {
            path: p.into(),
            is_directory: true,
        }
    }

    fn file(p: &str) -> Target {
        Target {
            path: p.into(),
            is_directory: false,
        }
    }

    #[test]
    fn a_terminal_prefers_windows_terminal_rooted_at_the_folder() {
        let r = terminal_request(
            &dir(r"C:\a b"),
            &found(&[("wt.exe", r"C:\wt.exe"), ("powershell.exe", r"C:\ps.exe")]),
        )
        .unwrap();
        assert_eq!(r"C:\wt.exe", r.file);
        assert_eq!(r#"-d "C:\a b""#, r.arguments);
    }

    #[test]
    fn a_file_opens_its_containing_folder() {
        let r = terminal_request(
            &file(r"C:\a\b.txt"),
            &found(&[("powershell.exe", r"C:\ps.exe")]),
        )
        .unwrap();
        assert_eq!(r"C:\ps.exe", r.file);
        assert_eq!(r"C:\a", r.working_directory);
        assert_eq!("", r.arguments);
    }

    #[test]
    fn no_terminal_at_all_is_said_in_words() {
        let e = terminal_request(&dir(r"C:\a"), &found(&[])).unwrap_err();
        assert!(e.contains("neither Windows Terminal nor PowerShell"), "{e}");
    }

    #[test]
    fn vs_code_steps_over_the_batch_shim_to_the_editor() {
        let r = vscode_request(
            &dir(r"C:\proj"),
            &found(&[
                (
                    "code",
                    r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd",
                ),
                (
                    r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe",
                    r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe",
                ),
            ]),
            None,
            None,
        )
        .unwrap();
        assert!(r.file.ends_with("Code.exe"), "{}", r.file);
        assert_eq!(r#""C:\proj""#, r.arguments);
    }

    #[test]
    fn vs_code_falls_back_to_the_standard_locations_then_the_launcher() {
        let by_location = vscode_request(
            &file(r"C:\a.txt"),
            &found(&[(
                r"C:\L\Programs\Microsoft VS Code\Code.exe",
                r"C:\L\Programs\Microsoft VS Code\Code.exe",
            )]),
            Some(r"C:\L"),
            None,
        )
        .unwrap();
        assert!(by_location.file.ends_with("Code.exe"));
        let launcher_only = vscode_request(
            &file(r"C:\a.txt"),
            &found(&[("code", r"C:\bin\code.cmd")]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(r"C:\bin\code.cmd", launcher_only.file);
        assert!(
            vscode_request(&file(r"C:\a.txt"), &found(&[]), None, None)
                .unwrap_err()
                .contains("VS Code not found")
        );
    }

    fn command(files: bool, dirs: bool, elevated: bool) -> CustomCommandDefinition {
        CustomCommandDefinition {
            id: "x".into(),
            show_in_menu: true,
            name: "Edit".into(),
            command: "notepad".into(),
            arguments: r#""{path}" --dir "{dir}""#.into(),
            applies_to_files: files,
            applies_to_directories: dirs,
            run_elevated: elevated,
        }
    }

    #[test]
    fn a_custom_command_runs_once_per_target_it_applies_to_with_the_template_expanded() {
        let targets = [file(r"C:\a\x.txt"), dir(r"C:\a\sub"), file(r"C:\a\y.txt")];
        let requests = custom_requests(
            &command(true, false, true),
            &targets,
            &found(&[("notepad", r"C:\Windows\notepad.exe")]),
        )
        .unwrap();
        assert_eq!(2, requests.len());
        assert_eq!(r"C:\Windows\notepad.exe", requests[0].file);
        assert!(
            requests[0].arguments.contains(r"C:\a\x.txt"),
            "{}",
            requests[0].arguments
        );
        assert_eq!(r"C:\a", requests[0].working_directory);
        assert!(requests.iter().all(|r| r.elevated));
    }

    #[test]
    fn a_folder_target_runs_in_the_folder_itself() {
        let requests = custom_requests(
            &command(false, true, false),
            &[dir(r"C:\a\sub")],
            &found(&[("notepad", r"C:\n.exe")]),
        )
        .unwrap();
        assert_eq!(r"C:\a\sub", requests[0].working_directory);
        assert!(!requests[0].elevated);
    }

    #[test]
    fn a_program_that_is_not_there_is_one_message_not_one_per_item() {
        let targets = [file(r"C:\a\x.txt"), file(r"C:\a\y.txt")];
        let e = custom_requests(&command(true, false, false), &targets, &found(&[])).unwrap_err();
        assert_eq!("'Edit' failed: 'notepad' was not found.", e);
    }

    #[test]
    fn a_command_that_applies_to_nothing_selected_does_nothing_quietly() {
        let r = custom_requests(
            &command(false, true, false),
            &[file(r"C:\a\x.txt")],
            &found(&[("notepad", "n")]),
        )
        .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn the_ids_the_host_hands_over() {
        for id in [
            "file.run-as-admin",
            "file.open-terminal",
            "file.open-vscode",
            "custom:abc",
        ] {
            assert!(is_launch_command(id), "{id}");
        }
        assert!(!is_launch_command("file.rename"));
    }
}
