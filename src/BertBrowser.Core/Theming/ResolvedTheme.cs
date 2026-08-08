namespace BertBrowser.Core.Theming;

/// <summary>
/// A theme with every token in <see cref="ThemeToken.All"/> resolved to a concrete colour. This is
/// what the UI consumes; by construction a lookup can never miss.
/// </summary>
public sealed class ResolvedTheme
{
    private readonly IReadOnlyDictionary<string, ThemeColor> _colors;

    internal ResolvedTheme(
        string id,
        string name,
        bool isDark,
        IReadOnlyDictionary<string, ThemeColor> colors,
        IReadOnlyList<ThemeIssue> issues)
    {
        Id = id;
        Name = name;
        IsDark = isDark;
        _colors = colors;
        Issues = issues;
    }

    public string Id { get; }
    public string Name { get; }
    public bool IsDark { get; }

    /// <summary>Problems found while resolving. Empty for a well-formed theme.</summary>
    public IReadOnlyList<ThemeIssue> Issues { get; }

    public IReadOnlyDictionary<string, ThemeColor> Colors => _colors;

    public ThemeColor this[string token] => _colors[token];
}
