using System.Text.Json;

namespace BertBrowser.Core.Theming;

/// <summary>
/// The on-disk format for user themes. Kept in Core so the round trip is unit-tested: this is the
/// only part of theming the user can edit with a text editor.
/// </summary>
public static class ThemeJson
{
    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        // Colours a future version adds are just extra properties to this one; ignoring unknown
        // members keeps a newer theme file loadable by an older build.
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    public static string Serialize(ThemeDefinition definition) =>
        JsonSerializer.Serialize(definition, Options);

    /// <summary>
    /// Writes every token out explicitly with no <c>base</c>, so an exported theme is portable and
    /// readable rather than a diff against whatever Dark+ happened to be at the time.
    /// </summary>
    public static string SerializeResolved(ResolvedTheme theme)
    {
        var colors = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var key in ThemeToken.All)
        {
            colors[key] = theme[key].ToHex();
        }

        return Serialize(new ThemeDefinition
        {
            Id = theme.Id,
            Name = theme.Name,
            IsDark = theme.IsDark,
            Colors = colors,
        });
    }

    /// <summary>Never throws: malformed JSON is a file the user typed into.</summary>
    public static bool TryDeserialize(string json, out ThemeDefinition? definition, out string? error)
    {
        definition = null;
        error = null;
        try
        {
            var parsed = JsonSerializer.Deserialize<ThemeDefinition>(json, Options);
            if (parsed is null)
            {
                error = "The file is empty.";
                return false;
            }
            if (string.IsNullOrWhiteSpace(parsed.Id))
            {
                error = "The theme has no \"id\".";
                return false;
            }
            definition = string.IsNullOrWhiteSpace(parsed.Name) ? WithName(parsed, parsed.Id) : parsed;
            return true;
        }
        catch (JsonException ex)
        {
            error = ex.Message;
            return false;
        }
    }

    private static ThemeDefinition WithName(ThemeDefinition definition, string name) => new()
    {
        Id = definition.Id,
        Name = name,
        BaseThemeId = definition.BaseThemeId,
        IsDark = definition.IsDark,
        Colors = definition.Colors,
        IsBuiltIn = definition.IsBuiltIn,
    };
}
