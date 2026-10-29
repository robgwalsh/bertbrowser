namespace BertBrowser.Core.Theming;

/// <param name="ThemeId">The theme to apply. Always usable — never null, never empty.</param>
/// <param name="Issue">What was wrong with the user's configuration, for the notice line in
/// Settings, or null when nothing was.</param>
public readonly record struct SystemThemeChoice(string ThemeId, ThemeIssue? Issue);

/// <summary>
/// Which theme "match Windows" means right now.
/// </summary>
/// <remarks>
/// Pure, so every row of the decision is a test rather than something to notice in a screenshot —
/// the same shape as <c>IndexerBannerRules</c>. <c>ThemeService</c> is WPF-coupled and has no tests,
/// so no branch of this may live there.
/// </remarks>
public static class SystemThemeRules
{
    public static string DefaultLightThemeId => ThemeCatalog.LightPlus.Id;

    public static string DefaultDarkThemeId => ThemeCatalog.DarkPlus.Id;

    /// <summary>What high contrast always resolves to. Our control templates are fully custom, so
    /// they never pick up Windows' own high-contrast colours; this is the closest equivalent.</summary>
    public static string HighContrastThemeId => ThemeCatalog.HighContrastDark.Id;

    /// <summary>
    /// Whether a launch should match Windows, given what the settings file said.
    /// </summary>
    /// <param name="configured">The stored preference; null when it has never been set.</param>
    /// <param name="hasSettingsFile">Whether settings came from a file at all.</param>
    /// <remarks>
    /// A brand-new install matches Windows; anyone who already had a settings file keeps what they
    /// had. It has to key off the file rather than off a null theme id, because someone who has
    /// simply never opened Settings also has no theme id — flipping all of them on upgrade would be
    /// exactly the surprise this is nullable to avoid. The caller pins the answer immediately, or
    /// launch two would find a file and read the null as "no".
    /// </remarks>
    public static bool ShouldFollow(bool? configured, bool hasSettingsFile) =>
        configured ?? !hasSettingsFile;

    /// <param name="lookup">Resolves an id to a theme, user themes included — a slot pointing at a
    /// theme the user deleted is the case that matters, and <see cref="ThemeCatalog.Find"/> alone
    /// would not see it.</param>
    public static SystemThemeChoice Decide(
        SystemAppearance appearance,
        string? lightThemeId,
        string? darkThemeId,
        Func<string, ThemeDefinition?> lookup)
    {
        // High contrast wins outright, and deliberately does not consult the slots at all: an
        // accessibility setting must not be able to land on a low-contrast theme because of
        // something configured months ago, and a broken slot must not produce a warning here.
        if (appearance.IsHighContrast) return new SystemThemeChoice(HighContrastThemeId, null);

        var (wanted, fallback, slot) = appearance.IsDark
            ? (darkThemeId, DefaultDarkThemeId, "dark")
            : (lightThemeId, DefaultLightThemeId, "light");

        if (string.IsNullOrWhiteSpace(wanted)) return new SystemThemeChoice(fallback, null);

        if (lookup(wanted) is not { } definition)
        {
            // The same reasoning as ThemeService.FindOrFallBack: a theme can legitimately be absent
            // for one launch — a file still syncing, a themes folder on a drive that isn't mounted —
            // so fall back for now and say so. The caller must not rewrite the setting.
            return new SystemThemeChoice(fallback, new ThemeIssue(
                ThemeIssueSeverity.Error, null,
                $"Your {slot} theme '{wanted}' was not found; using {fallback} for now."));
        }

        if (definition.IsDark != appearance.IsDark)
        {
            // Honoured rather than corrected. IsDark is metadata (see ThemeDefinition) and the
            // choice is the user's; the pickers only offer matching themes, so this can only come
            // from a hand-edited settings.json or a user theme whose own isDark changed later.
            return new SystemThemeChoice(definition.Id, new ThemeIssue(
                ThemeIssueSeverity.Warning, null,
                $"'{definition.Name}' is set as your {slot} theme but is not a {slot} theme."));
        }

        return new SystemThemeChoice(definition.Id, null);
    }

    /// <summary>
    /// What one slot's picker offers.
    /// </summary>
    /// <remarks>
    /// High contrast is excluded: the dark list answers "which dark theme do I like", and the
    /// accessibility theme arrives on its own switch rather than by being chosen here. Anyone who
    /// wants it permanently unticks the checkbox and picks it in the single picker.
    /// <para>
    /// <paramref name="assignedId"/> is kept in the list even when it fails the filter, or a slot
    /// holding a mismatched theme would render as an empty box and the only way to see what was set
    /// would be the settings file.
    /// </para>
    /// </remarks>
    public static IReadOnlyList<ThemeDefinition> SlotChoices(
        IReadOnlyList<ThemeDefinition> available, bool dark, string? assignedId = null)
    {
        var chosen = available
            .Where(t => t.IsDark == dark &&
                        !string.Equals(t.Id, HighContrastThemeId, StringComparison.OrdinalIgnoreCase))
            .ToList();

        if (assignedId is { Length: > 0 } &&
            !chosen.Any(t => string.Equals(t.Id, assignedId, StringComparison.OrdinalIgnoreCase)) &&
            available.FirstOrDefault(t =>
                string.Equals(t.Id, assignedId, StringComparison.OrdinalIgnoreCase)) is { } odd)
        {
            chosen.Insert(0, odd);
        }

        return chosen;
    }
}
