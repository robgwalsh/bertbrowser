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
    /// Registered, and pointing at the helper beside this build.
    On,
    /// Registered, but pointing at an executable that is no longer there: an update moved the
    /// install directory out from under it.
    Stale,
    /// It could not be read. Rendered as off, never as on: a ticked box nobody verified is a
    /// promise the app has not checked it can keep.
    Unknown,
}

pub struct IndexerAutoStartTask;

impl IndexerAutoStartTask {
    /// What the task is called, in the task scheduler's root folder.
    pub const TASK_NAME: &'static str = "BertBrowser Index Helper";

    /// The task definition, as Task Scheduler 1.2 XML. `data_directory` must be the user's real
    /// profile directory, never an overridden one: a harness run that registered a task would
    /// otherwise leave an elevated process started at every sign-in, pointed at a scratch directory
    /// that no longer exists. `user_sid` is who the task runs as, and it must be the current user
    /// rather than SYSTEM: the helper derives its pipe name from its own token, so a SYSTEM helper
    /// would listen on a name no app looks for.
    pub fn build_xml(helper_path: &str, data_directory: &str, user_sid: &str) -> String {
        let arguments = format!("--data-dir \"{}\"", data_directory.trim_end_matches('\\'));
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

    /// True when a registered task points somewhere other than where the helper now lives.
    /// Velopack replaces the install directory on an update, which leaves the task aimed at an
    /// executable that is gone; it cannot be repaired without a prompt, so the settings page says
    /// so rather than pretending.
    pub fn needs_reregistration(
        registered_command: Option<&str>,
        current_helper_path: &str,
    ) -> bool {
        let Some(registered) = registered_command else {
            return false;
        };
        if registered.trim().is_empty() {
            return false;
        }
        let registered = PathKey::canonicalize(registered.trim_matches('"'));
        let current = PathKey::canonicalize(current_helper_path);
        match (registered, current) {
            (Ok(r), Ok(c)) => r != c,
            // A command that does not even canonicalise cannot be the helper.
            _ => true,
        }
    }

    /// What the scheduler holds, decided from the task file's XML (`%WINDIR%\System32\Tasks\<name>`):
    /// `None` when there is no file, the text when there is. Reading the file is the App's; this
    /// is the judgement. A file that cannot be read is `Unknown` on the caller's side.
    pub fn state_from_task_xml(
        task_xml: Option<&str>,
        current_helper_path: &str,
    ) -> AutoStartState {
        let Some(xml) = task_xml else {
            return AutoStartState::Off;
        };
        let command = Self::first_element_text(xml, "Command").map(Self::unescape);
        if Self::needs_reregistration(command.as_deref(), current_helper_path) {
            AutoStartState::Stale
        } else {
            AutoStartState::On
        }
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
