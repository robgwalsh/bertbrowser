//! The elevated indexer's command line: what to do, and where the database is. Pure, like
//! `CommandLine` is, so every rule here is testable without a process to launch. Spec:
//! Core/Ipc/IndexerArguments.cs.
//!
//! **It is no longer told which pipe to call back on, and that is the point.** The name is derived
//! from this process's own token (`IndexEndpoint`), so the one argument that used to let a caller
//! aim the elevated process at an endpoint of their choosing does not exist any more, a stronger
//! property than validating it was. The parent process id went with it: the helper outlives whoever
//! launched it, so there is no parent to expect or to watch.
//!
//! **The data directory is still passed and still checked.** It is genuinely not derivable here: it
//! honours an override the UI harness relies on so a scripted run cannot touch the user's real
//! index, and working it out on this side would make that isolation depend on environment
//! inheritance across an elevation boundary. So it arrives from a process this one does not control
//! and is checked the way any other inbound path is.

use crate::cli::NavigationRequest;

/// What the index helper was started to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IndexerCommand {
    /// Index, and serve apps as they come and go. The ordinary case.
    #[default]
    Run,
    /// Register the sign-in task, then exit.
    RegisterAutoStart,
    /// Remove the sign-in task, then exit.
    UnregisterAutoStart,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexerArguments {
    pub command: IndexerCommand,
    pub data_directory: String,
}

impl IndexerArguments {
    /// Parses, or explains why not. The error is for a log; nobody types this command line.
    pub fn try_parse<S: AsRef<str>>(args: &[S]) -> Result<IndexerArguments, String> {
        let mut data_dir: Option<&str> = None;
        let mut command: Option<IndexerCommand> = None;

        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_ref();
            let value = args.get(i + 1).map(|v| v.as_ref());
            match (arg, value) {
                ("--data-dir", Some(v)) => {
                    data_dir = Some(v);
                    i += 1;
                }
                ("--register-autostart" | "--unregister-autostart", _) => {
                    if command.is_some() {
                        return Err(
                            "Only one of --register-autostart and --unregister-autostart may be given."
                                .to_string(),
                        );
                    }
                    command = Some(if arg == "--register-autostart" {
                        IndexerCommand::RegisterAutoStart
                    } else {
                        IndexerCommand::UnregisterAutoStart
                    });
                }
                _ => {
                    // An unrecognised option is an error, never a positional value: the same rule
                    // the user-facing command line follows, and for the same reason. --pipe and
                    // --parent-pid land here now, which is what makes their removal real rather
                    // than a validator nothing calls.
                    return Err(format!("Unrecognised argument: {arg}"));
                }
            }
            i += 1;
        }

        let Some(data_dir) = data_dir else {
            return Err(
                "Usage: BertBrowser.Indexer [--register-autostart|--unregister-autostart] --data-dir <path>"
                    .to_string(),
            );
        };
        if !NavigationRequest::is_acceptable_path(data_dir) {
            return Err("The data directory is not an acceptable absolute path.".to_string());
        }

        Ok(IndexerArguments {
            command: command.unwrap_or_default(),
            data_directory: data_dir.to_string(),
        })
    }
}
