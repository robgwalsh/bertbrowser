//! Something wrong with a theme, reported rather than thrown. Theme files are user-editable text,
//! so resolution always produces a complete, usable theme and hands the problems back to be shown
//! in Settings. Spec: src/BertBrowser.Core/Theming/ThemeIssue.cs.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeIssueSeverity {
    /// Resolution carried on and the result is usable; something was ignored or substituted.
    Warning,
    /// The theme the user asked for could not be used at all and something else was substituted.
    Error,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ThemeIssue {
    pub severity: ThemeIssueSeverity,
    /// The token at fault, or `None` when the problem is with the theme as a whole.
    pub token: Option<String>,
    pub message: String,
}

impl ThemeIssue {
    pub fn new(severity: ThemeIssueSeverity, token: Option<&str>, message: String) -> ThemeIssue {
        ThemeIssue {
            severity,
            token: token.map(str::to_string),
            message,
        }
    }
}
