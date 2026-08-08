using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// Owns the active theme. Applying one recolours the shared brushes in
/// <see cref="ThemeTokenDictionary"/>, so the whole UI updates without a restart and without
/// anything having to rebind.
/// </summary>
public interface IThemeService
{
    ResolvedTheme Current { get; }

    /// <summary>Built-in themes followed by the user's own, in display order.</summary>
    IReadOnlyList<ThemeDefinition> Available { get; }

    /// <summary>Problems with the current selection — a missing theme file, a colour that didn't
    /// parse — for Settings to show. Empty when all is well.</summary>
    IReadOnlyList<ThemeIssue> Issues { get; }

    /// <summary>How far a hidden item's icon is dimmed; 0.45 reads as mud on a dark background.</summary>
    double DimmedIconOpacity { get; }

    /// <summary>Raised after the palette has been recoloured.</summary>
    event EventHandler? ThemeChanged;

    /// <summary>Loads the user's themes and applies the persisted selection. Call once during
    /// startup, before the first window is created.</summary>
    void Initialize();

    /// <summary>Applies a theme and persists the choice.</summary>
    void SelectTheme(string themeId);

    /// <summary>Rescans the user's themes folder.</summary>
    void ReloadAvailableThemes();

    ThemeColor GetColor(string token);

    bool IsOverridden(string token);

    /// <summary>The value this token would have without the user's override — what "reset" restores.</summary>
    ThemeColor GetThemeColor(string token);

    /// <summary>Applies an unsaved colour change immediately; null clears the override.</summary>
    void SetOverride(string token, ThemeColor? color);

    /// <summary>Writes the current overrides to settings.json.</summary>
    void PersistOverrides();

    /// <summary>Discards every override made since the last <see cref="PersistOverrides"/>.</summary>
    void RevertOverrides();

    /// <summary>Clears all customisation of the current theme and persists that.</summary>
    void ResetAllOverrides();

    /// <summary>Bakes the current theme plus its overrides into a new user theme and selects it.</summary>
    bool TrySaveAsNewTheme(string name, out ThemeDefinition? created, out string? error);

    /// <summary>Copies a theme file into the user's themes folder.</summary>
    bool TryImport(string sourcePath, out ThemeDefinition? imported, out string? error);

    /// <summary>Writes the current theme out in full, with no base, so the file stands alone.</summary>
    bool TryExport(string destinationPath, out string? error);

    bool TryDeleteUserTheme(string themeId, out string? error);
}
