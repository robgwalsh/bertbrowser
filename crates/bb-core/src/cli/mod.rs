//! Command-line parsing and the single-instance wire format, with the one rule deciding whether a
//! path may be acted on at all. Spec: Core/Cli/CommandLine.cs, Core/Cli/NavigationRequest.cs.

mod command_line;
mod navigation_request;

pub use command_line::CommandLine;
pub use navigation_request::NavigationRequest;

/// Where a request wants its folders opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OpenIn {
    /// Whatever the app would do anyway: a tab in the active pane.
    #[default]
    Default,
    /// Force a new tab, even for a folder already showing.
    NewTab,
    /// A pane of its own, beside the active one. This app splits panes rather than opening second
    /// windows.
    NewPane,
}

impl OpenIn {
    pub fn name(self) -> &'static str {
        match self {
            OpenIn::Default => "Default",
            OpenIn::NewTab => "NewTab",
            OpenIn::NewPane => "NewPane",
        }
    }

    /// Exact, case-sensitive, as the wire format writes it.
    pub fn parse(s: &str) -> Option<OpenIn> {
        match s {
            "Default" => Some(OpenIn::Default),
            "NewTab" => Some(OpenIn::NewTab),
            "NewPane" => Some(OpenIn::NewPane),
            _ => None,
        }
    }
}

/// The folder to show, or with `select` the item to highlight inside its parent (Explorer's
/// `/select`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTarget {
    pub path: String,
    pub select: bool,
}

impl OpenTarget {
    pub fn new(path: &str, select: bool) -> OpenTarget {
        OpenTarget {
            path: path.to_string(),
            select,
        }
    }
}

/// `errors` holds anything that could not be understood. Reported rather than guessed at: an
/// unrecognised option must never be silently treated as a path.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CommandLineRequest {
    pub targets: Vec<OpenTarget>,
    pub mode: OpenIn,
    pub errors: Vec<String>,
}

impl CommandLineRequest {
    pub fn has_targets(&self) -> bool {
        !self.targets.is_empty()
    }
}
