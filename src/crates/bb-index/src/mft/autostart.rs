//! The Windows scheduled task that starts the index helper at sign-in, so it is already running,
//! and already elevated, before the app is opened. A task registered with `HighestAvailable` runs
//! elevated without a prompt. Registering it needs elevation once, which is exactly the gesture that
//! should stand in front of "install something that runs with an administrator token at every
//! sign-in": the app does it by running the helper through the same `runas` path it starts it with,
//! never by a verb on the pipe, which would let anything reaching the pipe install that persistence
//! with no gesture at all. Pure; the COM registration is the helper exe's (`bb-indexer`). Spec:
//! Core/Services/Mft/IndexerAutoStartTask.cs, App/Services/Indexing/IndexAutoStartService.cs.

use bb_paths::PathKey;

/// Whether the index helper starts at sign-in, and whether that can be established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoStartState {
    /// No sign-in task is registered.
    Off,
    /// Registered, running the administrator-only installed copy, which is this build's helper.
    On,
    /// Registered, but not as `On` describes: an older registration that still points beside the
    /// app, another app folder, or an installed copy that is missing or differs from this build's
    /// helper (an update replaced the app). Re-registering repairs it, with a prompt.
    Stale,
    /// It could not be read. Rendered as off, never as on: a ticked box nobody verified is a
    /// promise the app has not checked it can keep.
    Unknown,
}

pub struct IndexerAutoStartTask;

impl IndexerAutoStartTask {
    /// What the task is called, in the task scheduler's root folder.
    pub const TASK_NAME: &'static str = "BertBrowser Index Helper";

    /// The task definition, as Task Scheduler 1.2 XML. `helper_path` is the administrator-only
    /// installed copy (`InstalledHelper`), never a file beside the app. `data_directory` must be the
    /// user's real profile directory, never an overridden one: a harness run that registered a task
    /// would otherwise leave an elevated process started at every sign-in, pointed at a scratch
    /// directory that no longer exists. `app_directory` is the app the helper belongs to. `user_sid`
    /// is who the task runs as, and it must be the current user rather than SYSTEM: the helper
    /// derives its pipe name from its own token, so a SYSTEM helper would listen on a name no app
    /// looks for.
    pub fn build_xml(
        helper_path: &str,
        data_directory: &str,
        app_directory: &str,
        user_sid: &str,
    ) -> String {
        let arguments = format!(
            "--data-dir \"{}\" --app-dir \"{}\"",
            data_directory.trim_end_matches('\\'),
            app_directory.trim_end_matches('\\')
        );
        let task = Self::escape(Self::TASK_NAME);
        let sid = Self::escape(user_sid);
        let command = Self::escape(helper_path);
        let arguments = Self::escape(&arguments);

        // The element order is fixed by the Task Scheduler schema: a misplaced one is rejected
        // outright with a line and column and nothing else. DisallowStartOnRemoteAppSession and
        // UseUnifiedSchedulingEngine are deliberately absent: they need schema 1.3, this declares
        // 1.2 for the widest compatibility, and neither buys anything here.
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Keeps BertBrowser's search index up to date. Started at sign-in so BertBrowser never has to ask for administrator rights.</Description>
    <URI>\{task}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{sid}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>"#
        )
    }

    /// What the scheduler holds, decided from the task file's XML (`%WINDIR%\System32\Tasks\<name>`):
    /// `None` when there is no file, the text when there is. Reading the file is the App's; this
    /// is the judgement. A file that cannot be read is `Unknown` on the caller's side.
    ///
    /// `On` needs all of: the task's command is `installed_helper_path`; its `--app-dir` is
    /// `app_directory`; and `installed_copy_matches` (the installed copy exists and is byte-identical
    /// to the helper beside this app). Anything else registered, including every registration that
    /// still points beside the app, is `Stale`.
    pub fn state_from_task_xml(
        task_xml: Option<&str>,
        installed_helper_path: &str,
        app_directory: &str,
        installed_copy_matches: bool,
    ) -> AutoStartState {
        let Some(xml) = task_xml else {
            return AutoStartState::Off;
        };
        let command = Self::first_element_text(xml, "Command").map(Self::unescape);
        let arguments = Self::first_element_text(xml, "Arguments").map(Self::unescape);
        let command_ok = command
            .as_deref()
            .is_some_and(|c| Self::same_path(c.trim().trim_matches('"'), installed_helper_path));
        let app_dir_ok = arguments
            .as_deref()
            .and_then(|a| Self::quoted_argument(a, "--app-dir"))
            .is_some_and(|dir| Self::same_path(dir, app_directory));
        if command_ok && app_dir_ok && installed_copy_matches {
            AutoStartState::On
        } else {
            AutoStartState::Stale
        }
    }

    fn same_path(a: &str, b: &str) -> bool {
        matches!((PathKey::canonicalize(a), PathKey::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
    }

    /// The value of `name "value"` in a command line. Paths here cannot contain a quote.
    fn quoted_argument<'a>(arguments: &'a str, name: &str) -> Option<&'a str> {
        let token = format!("{name} \"");
        let mut from = 0;
        while let Some(found) = arguments[from..].find(&token) {
            let at = from + found;
            if at == 0 || arguments.as_bytes()[at - 1] == b' ' {
                let start = at + token.len();
                let end = arguments[start..].find('"')? + start;
                return Some(&arguments[start..end]);
            }
            from = at + token.len();
        }
        None
    }

    /// XML-escapes a value: a profile path may perfectly well contain an ampersand, and an
    /// unescaped one makes the whole definition unparseable.
    pub fn escape(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for c in value.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
        out
    }

    /// The reverse, for reading a value back out of a task file.
    pub fn unescape(value: &str) -> String {
        value
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }

    /// The raw text of the first `<name>…</name>` element, namespace prefixes ignored.
    pub fn first_element_text<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find(&close)? + start;
        Some(&xml[start..end])
    }
}
