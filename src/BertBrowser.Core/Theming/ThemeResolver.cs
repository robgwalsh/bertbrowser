namespace BertBrowser.Core.Theming;

/// <summary>
/// Turns an authored <see cref="ThemeDefinition"/> into a complete <see cref="ResolvedTheme"/>.
/// </summary>
/// <remarks>
/// Resolution never throws. Theme files are hand-editable text and a base theme can legitimately be
/// missing (a file still syncing, a theme deleted while selected), so every problem becomes a
/// <see cref="ThemeIssue"/> and the caller still gets a usable theme. Colours are layered lowest to
/// highest: the Dark+ root, then the base chain from its far end inwards, then the theme's own
/// colours, then the user's per-token overrides.
/// </remarks>
public static class ThemeResolver
{
    /// <summary>Stands in for a token whose literal could not be parsed — deliberately unmissable.</summary>
    private static readonly ThemeColor Unparseable = ThemeColor.FromRgb(0xFF, 0x00, 0xFF);

    public static ResolvedTheme Resolve(
        ThemeDefinition definition,
        Func<string, ThemeDefinition?>? lookup = null,
        IReadOnlyDictionary<string, string>? overrides = null)
    {
        var issues = new List<ThemeIssue>();
        var colors = new Dictionary<string, ThemeColor>(ThemeToken.All.Count, StringComparer.Ordinal);

        // Seed from the Dark+ root so the result is complete whatever the theme leaves out — that is
        // what lets ResolvedTheme's indexer be total.
        foreach (var (key, literal) in ThemeCatalog.DarkPlus.Colors)
        {
            colors[key] = ThemeColor.TryParse(literal, out var color) ? color : Unparseable;
        }

        // Applied far end first, so a theme's own colours win over the base it inherits from.
        var chain = BuildChain(definition, lookup, issues);
        for (var i = chain.Count - 1; i >= 0; i--)
        {
            ApplyLayer(chain[i].Colors, colors, issues, $"theme '{chain[i].Id}'");
        }

        if (overrides is { Count: > 0 })
        {
            ApplyLayer(overrides, colors, issues, "your customisations");
        }

        return new ResolvedTheme(definition.Id, definition.Name, definition.IsDark, colors, issues);
    }

    /// <summary>
    /// Drops overrides that name an unknown token or that just restate the theme's own value, so the
    /// settings file doesn't accumulate dead entries as themes change between versions.
    /// </summary>
    public static Dictionary<string, string> PruneOverrides(
        IReadOnlyDictionary<string, string> overrides,
        ResolvedTheme themeWithoutOverrides)
    {
        var pruned = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (key, literal) in overrides)
        {
            if (!ThemeToken.IsKnown(key)) continue;
            if (!ThemeColor.TryParse(literal, out var color)) continue;
            if (themeWithoutOverrides[key] == color) continue;
            pruned[key] = color.ToHex();
        }
        return pruned;
    }

    private static List<ThemeDefinition> BuildChain(
        ThemeDefinition definition,
        Func<string, ThemeDefinition?>? lookup,
        List<ThemeIssue> issues)
    {
        var chain = new List<ThemeDefinition> { definition };
        var visited = new HashSet<string>(StringComparer.OrdinalIgnoreCase) { definition.Id };

        var current = definition;
        while (current.BaseThemeId is { Length: > 0 } baseId)
        {
            if (!visited.Add(baseId))
            {
                issues.Add(new ThemeIssue(ThemeIssueSeverity.Error, null,
                    $"Theme '{definition.Id}' inherits from itself via '{baseId}'; the loop was ignored."));
                break;
            }

            if (lookup?.Invoke(baseId) is not { } parent)
            {
                issues.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                    $"Base theme '{baseId}' was not found; using {ThemeCatalog.DarkPlus.Name} for the colours '{current.Id}' does not set."));
                break;
            }

            chain.Add(parent);
            current = parent;
        }

        return chain;
    }

    private static void ApplyLayer(
        IReadOnlyDictionary<string, string> layer,
        Dictionary<string, ThemeColor> colors,
        List<ThemeIssue> issues,
        string source)
    {
        foreach (var (key, literal) in layer)
        {
            if (!ThemeToken.IsKnown(key))
            {
                issues.Add(new ThemeIssue(ThemeIssueSeverity.Warning, key,
                    $"'{key}' in {source} is not a colour this version knows about; it was ignored."));
                continue;
            }

            if (!ThemeColor.TryParse(literal, out var color))
            {
                issues.Add(new ThemeIssue(ThemeIssueSeverity.Warning, key,
                    $"'{literal}' in {source} is not a colour; '{key}' kept its inherited value."));
                continue;
            }

            colors[key] = color;
        }
    }
}
