namespace BertBrowser.Core.Theming;

public enum ThemeIssueSeverity
{
    /// <summary>Resolution carried on and the result is usable; something was ignored or substituted.</summary>
    Warning,

    /// <summary>The theme the user asked for could not be used at all and something else was substituted.</summary>
    Error,
}

/// <summary>
/// Something wrong with a theme, reported rather than thrown. Theme files are user-editable text, so
/// resolution always produces a complete, usable theme and hands the problems back to be shown in
/// Settings.
/// </summary>
/// <param name="Token">The token at fault, or null when the problem is with the theme as a whole.</param>
public sealed record ThemeIssue(ThemeIssueSeverity Severity, string? Token, string Message);
