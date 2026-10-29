using System.Text.Json.Serialization;

namespace BertBrowser.Core.Theming;

/// <summary>
/// A theme as authored: an identity plus a — possibly sparse — set of colour overrides. Anything it
/// leaves out comes from <see cref="BaseThemeId"/>, so a user theme can be three lines long.
/// This is also the on-disk shape of a <c>*.json</c> file in the user's themes folder.
/// </summary>
public sealed class ThemeDefinition
{
    [JsonPropertyName("id")]
    public string Id { get; init; } = "";

    [JsonPropertyName("name")]
    public string Name { get; init; } = "";

    /// <summary>Theme to inherit unset colours from. Null for the two built-in roots.</summary>
    [JsonPropertyName("base")]
    public string? BaseThemeId { get; init; }

    /// <summary>
    /// Whether this reads as a dark theme. Metadata only — it selects the OS dark-mode window frame
    /// and how far hidden items are dimmed; it never affects colour resolution.
    /// </summary>
    [JsonPropertyName("isDark")]
    public bool IsDark { get; init; }

    /// <summary>Token key (see <see cref="ThemeToken"/>) to colour literal, e.g. <c>"#1E1E1E"</c>.</summary>
    [JsonPropertyName("colors")]
    public IReadOnlyDictionary<string, string> Colors { get; init; } =
        new Dictionary<string, string>(StringComparer.Ordinal);

    /// <summary>False for themes loaded from the user's themes folder, which can be edited and deleted.</summary>
    [JsonIgnore]
    public bool IsBuiltIn { get; init; }

    public ThemeDefinition WithColors(IReadOnlyDictionary<string, string> colors) => new()
    {
        Id = Id,
        Name = Name,
        BaseThemeId = BaseThemeId,
        IsDark = IsDark,
        Colors = colors,
        IsBuiltIn = IsBuiltIn,
    };
}
