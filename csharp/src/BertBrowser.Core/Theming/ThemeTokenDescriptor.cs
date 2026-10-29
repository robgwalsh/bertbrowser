namespace BertBrowser.Core.Theming;

/// <summary>
/// Editor-facing metadata for one colour token. <paramref name="Key"/> is the only identity that
/// matters — it is simultaneously the JSON property name, the WPF resource key, and what the theme
/// editor writes back — so there is exactly one name for a colour across the whole app.
/// </summary>
/// <param name="IsCore">
/// True for the handful of colours worth showing before the user asks for "all colours". Everything
/// else is still editable, just not in the default list.
/// </param>
public sealed record ThemeTokenDescriptor(
    string Key,
    string Group,
    string DisplayName,
    string Description,
    bool IsCore);
