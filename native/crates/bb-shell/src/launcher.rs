//! The one place this app starts another program on the user's behalf (`ShellExecuteExW`). The
//! elevated helpers are the only other thing started, and they are started by
//! `bb_core::elevation::elevated_process`; a test in this crate fails if any other source file in
//! the product mentions a way to start a process.
//! Spec: App/Services/ProcessLauncher.cs.
//!
//! Still a single chokepoint, though no longer for the original reason: this process is
//! `asInvoker`, a child inherits an ordinary token, and "Run as administrator" is `runas` from a
//! medium-integrity process, which raises a real UAC prompt. What remains worth keeping is one
//! place to audit, to fake in the harness, and to change. **Nothing starts elevated unless the user
//! chose it.**

use std::sync::Arc;

use bb_core::launch::ExecutablePath;
use bb_core::shell_integration::{ElevatedOpen, RunAsVerbRules};

use crate::registry::{Registry, SystemRegistry};
use crate::run_as::RunAsVerbRegistry;

/// What to start. `arguments` and `working_directory` may be empty.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LaunchRequest {
    pub file: String,
    pub arguments: String,
    pub working_directory: String,
    /// The user chose "run as administrator": `runas`, with a UAC prompt.
    pub elevated: bool,
}

impl LaunchRequest {
    pub fn open(file: &str) -> LaunchRequest {
        LaunchRequest {
            file: file.to_string(),
            ..LaunchRequest::default()
        }
    }
}

/// What the request comes to once the elevated case has been decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub target: String,
    pub arguments: String,
    pub working_directory: String,
    pub run_as: bool,
}

const ERROR_CANCELLED: u32 = 1223;
const ERROR_NO_ASSOCIATION: u32 = 1155;

/// What to call this file in a sentence.
pub fn describe(file: &str) -> String {
    let trimmed = file.trim_end_matches('\\');
    match trimmed.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => file.to_string(),
    }
}

/// Decides what is started. `decide` is the run-as lookup for the file; only asked when elevated.
/// `Err` is the message for the status line.
pub fn plan(
    request: &LaunchRequest,
    decide: impl Fn(&str) -> ElevatedOpen,
) -> Result<LaunchPlan, String> {
    if request.file.trim().is_empty() {
        return Err("Nothing to open.".to_string());
    }

    let mut target = request.file.clone();
    let mut arguments = request.arguments.clone();
    if request.elevated {
        // Decided here rather than only at the menu, so every route gets the same answer: the
        // keyboard shortcut, and a custom command with the elevated box ticked, neither of which
        // passes the context menu's check.
        match decide(&request.file) {
            ElevatedOpen::None => {
                return Err(RunAsVerbRules::cannot_run_message(&describe(&request.file)));
            }
            // No runas verb, but the file has a handler: start that elevated with the file as its
            // argument, which is what running a .sln or a config file as administrator means. Any
            // arguments a caller passed are dropped, deliberately: they were meant for the file,
            // and this is a different program.
            ElevatedOpen::Handler {
                executable,
                arguments: handler_arguments,
            } => {
                target = executable;
                arguments = handler_arguments;
            }
            ElevatedOpen::Verb => {}
        }
    }

    Ok(LaunchPlan {
        target,
        arguments,
        working_directory: request.working_directory.clone(),
        run_as: request.elevated,
    })
}

/// The status-line words for a `ShellExecuteExW` failure.
pub fn failure_message(error: u32, file: &str, elevated: bool) -> String {
    match error {
        ERROR_CANCELLED => format!("'{}' was not opened.", describe(file)),
        // Windows saying there is no runas verb for this file. The pre-check catches almost every
        // case, but not a shortcut, which is offered on purpose because the registry cannot say
        // what it points at. Say the useful thing rather than "No application is associated with
        // the specified file for this operation".
        ERROR_NO_ASSOCIATION if elevated => RunAsVerbRules::cannot_run_message(&describe(file)),
        other => format!(
            "Cannot open: {}",
            std::io::Error::from_raw_os_error(other as i32)
        ),
    }
}

pub struct SystemLauncher {
    run_as: RunAsVerbRegistry,
}

impl SystemLauncher {
    pub fn new() -> SystemLauncher {
        SystemLauncher::over(Arc::new(SystemRegistry))
    }

    pub fn over(registry: Arc<dyn Registry>) -> SystemLauncher {
        SystemLauncher {
            run_as: RunAsVerbRegistry::new(registry),
        }
    }

    /// Starts the request as the logged-on user, or as administrator when `elevated`. `Err` is a
    /// message for the status bar.
    pub fn start(&self, request: &LaunchRequest) -> Result<(), String> {
        let plan = plan(request, |file| self.run_as.decide(file, false, false))?;
        shell_execute(&plan).map_err(|e| failure_message(e, &request.file, request.elevated))
    }

    /// The absolute path `program` resolves to against `PATH`, or None. It also decides *what* runs,
    /// rather than leaving a bare name to be resolved later against whatever folder is current.
    pub fn resolve(&self, program: &str) -> Option<String> {
        ExecutablePath::resolve_on_disk(program)
    }

    /// Whether "Run as administrator" can do anything for this file.
    pub fn can_run_elevated(&self, path: &str) -> bool {
        self.run_as.can_run_elevated(path, false, false)
    }
}

impl Default for SystemLauncher {
    fn default() -> Self {
        SystemLauncher::new()
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `Err` is the Win32 error code.
fn shell_execute(plan: &LaunchPlan) -> Result<(), u32> {
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = wide("runas");
    let file = wide(&plan.target);
    let parameters = wide(&plan.arguments);
    let directory = wide(&plan.working_directory);

    // SAFETY: a handler may use COM, so the calling thread needs an apartment for the call; a thread
    // that already has one (S_FALSE) or a different one (RPC_E_CHANGED_MODE) is left as it is.
    let com = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    let initialised = com >= 0;

    // SAFETY: a plain-old-data struct, zeroed then filled; every pointer outlives the call.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = if plan.run_as { verb.as_ptr() } else { null() };
    info.lpFile = file.as_ptr();
    info.lpParameters = if plan.arguments.is_empty() {
        null()
    } else {
        parameters.as_ptr()
    };
    info.lpDirectory = if plan.working_directory.is_empty() {
        null()
    } else {
        directory.as_ptr()
    };
    info.nShow = SW_SHOWNORMAL;

    // SAFETY: `info` is fully initialised and the strings it points at are alive.
    let ok = unsafe { ShellExecuteExW(&mut info) };
    let result = if ok == 0 {
        // SAFETY: reading the thread's last error.
        Err(unsafe { GetLastError() })
    } else {
        if !info.hProcess.is_null() {
            // SAFETY: a process handle ShellExecuteEx handed back; nobody waits on it here.
            unsafe { CloseHandle(info.hProcess) };
        }
        Ok(())
    };

    if initialised {
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_handler(_: &str) -> ElevatedOpen {
        ElevatedOpen::None
    }

    #[test]
    fn nothing_to_open_is_a_message_not_a_launch() {
        assert_eq!(
            Err("Nothing to open.".to_string()),
            plan(&LaunchRequest::open("  "), no_handler)
        );
    }

    #[test]
    fn an_ordinary_launch_passes_everything_through_unelevated() {
        let request = LaunchRequest {
            file: r"C:\x\a.txt".into(),
            arguments: "--flag".into(),
            working_directory: r"C:\x".into(),
            elevated: false,
        };
        let p = plan(&request, |_| panic!("not asked when not elevated")).unwrap();
        assert_eq!(
            LaunchPlan {
                target: r"C:\x\a.txt".into(),
                arguments: "--flag".into(),
                working_directory: r"C:\x".into(),
                run_as: false
            },
            p
        );
    }

    #[test]
    fn elevated_with_a_verb_is_runas_on_the_file_itself() {
        let request = LaunchRequest {
            file: r"C:\x\a.exe".into(),
            elevated: true,
            ..LaunchRequest::default()
        };
        let p = plan(&request, |_| ElevatedOpen::Verb).unwrap();
        assert!(p.run_as);
        assert_eq!(r"C:\x\a.exe", p.target);
    }

    #[test]
    fn elevated_with_a_handler_starts_the_handler_and_drops_the_callers_arguments() {
        let request = LaunchRequest {
            file: r"C:\x\a.sln".into(),
            arguments: "ignored".into(),
            elevated: true,
            ..LaunchRequest::default()
        };
        let p = plan(&request, |_| {
            ElevatedOpen::handler(r"C:\ide.exe", r#""C:\x\a.sln""#)
        })
        .unwrap();
        assert_eq!(
            (r"C:\ide.exe", r#""C:\x\a.sln""#),
            (p.target.as_str(), p.arguments.as_str())
        );
        assert!(p.run_as);
    }

    #[test]
    fn elevated_with_nothing_to_run_says_so_in_words() {
        let request = LaunchRequest {
            file: r"C:\x\a.txt".into(),
            elevated: true,
            ..LaunchRequest::default()
        };
        let message = plan(&request, no_handler).unwrap_err();
        assert!(message.contains("a.txt"), "{message}");
    }

    #[test]
    fn a_declined_prompt_is_the_users_choice_not_a_failure() {
        assert_eq!(
            "'a.exe' was not opened.",
            failure_message(1223, r"C:\x\a.exe", true)
        );
        assert!(failure_message(1155, r"C:\x\a.txt", true).contains("no program Windows can run"));
        assert!(failure_message(1155, r"C:\x\a.txt", false).starts_with("Cannot open:"));
    }

    #[test]
    fn describe_names_the_leaf_of_a_folder_with_a_trailing_slash() {
        assert_eq!("b", describe(r"C:\a\b\"));
        assert_eq!("C:", describe(r"C:\"));
    }

    /// The rule the whole module exists for: a single process-start call site. Everything that
    /// starts a program goes through `SystemLauncher`; the elevated helpers go through
    /// `bb_core::elevation::elevated_process`; and the product's other source files never name a
    /// way to start one.
    #[test]
    fn nothing_else_in_the_product_starts_a_process() {
        fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path
                        .file_name()
                        .is_some_and(|n| n == "tests" || n == "target")
                    {
                        continue;
                    }
                    visit(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // Only the code before the test module counts.
                    let code: String = text
                        .split("#[cfg(test)]")
                        .next()
                        .unwrap_or("")
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("//"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    for needle in [
                        "Command::new(",
                        "CreateProcessW",
                        "CreateProcessA",
                        "ShellExecuteW",
                        "ShellExecuteExW",
                        "WinExec",
                    ] {
                        if code.contains(needle) {
                            found.push(format!("{}: {needle}", path.display()));
                        }
                    }
                }
            }
        }

        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut found = Vec::new();
        for name in ["bb-app", "bb-core", "bb-ffi", "bb-shell", "bb-paths"] {
            visit(&crates.join(name).join("src"), &mut found);
        }
        let allowed = [
            "bb-shell\\src\\launcher.rs",
            "bb-shell/src/launcher.rs",
            "bb-core\\src\\elevation\\launcher.rs",
            "bb-core/src/elevation/launcher.rs",
        ];
        let strays: Vec<&String> = found
            .iter()
            .filter(|f| !allowed.iter().any(|a| f.contains(a)))
            .collect();
        assert!(
            strays.is_empty(),
            "a second way to start a process: {strays:?}"
        );
    }
}
