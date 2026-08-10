using System.IO;
using System.Windows;
using BertBrowser.App.Services;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// Deliberately thin. Everything worth testing — inheritance, fallbacks, validation — lives in
/// <see cref="ThemeResolver"/> in Core; this only decides which definition to resolve, hands the
/// result to the palette, and remembers the choice.
/// </summary>
public sealed class ThemeService : IThemeService
{
    private readonly AppSettings _settings;
    private readonly UserThemeStore _store;

    private List<ThemeDefinition> _available = new(ThemeCatalog.BuiltIns);
    private IReadOnlyList<ThemeIssue> _storeIssues = Array.Empty<ThemeIssue>();
    private Dictionary<string, string> _overrides = new(StringComparer.Ordinal);
    private Dictionary<string, string> _persistedOverrides = new(StringComparer.Ordinal);
    private ThemeDefinition _definition = ThemeCatalog.Default;

    public ThemeService(AppSettings settings, UserThemeStore store)
    {
        _settings = settings;
        _store = store;
        Current = ThemeResolver.Resolve(ThemeCatalog.Default, ThemeCatalog.Find);
        Issues = Array.Empty<ThemeIssue>();
    }

    public ResolvedTheme Current { get; private set; }

    public IReadOnlyList<ThemeDefinition> Available => _available;

    public IReadOnlyList<ThemeIssue> Issues { get; private set; }

    public double DimmedIconOpacity => Current.IsDark ? 0.55 : 0.45;

    public event EventHandler? ThemeChanged;

    public void Initialize()
    {
        // A merged dictionary loaded from a Source is realised on first lookup, so touch a token to
        // guarantee ThemeTokenDictionary exists before we try to recolour it. Without this a chosen
        // theme could silently fail to apply and leave the app on the parse-time defaults.
        _ = Application.Current?.TryFindResource(ThemeToken.WindowBackground);

        ReloadAvailableThemes();

        var id = _settings.ThemeId ?? DefaultForThisMachine();
        LoadOverrides(id);
        Apply(FindOrFallBack(id));
    }

    /// <summary>
    /// What to use before the user has chosen. Our control templates are fully custom, so they do
    /// not pick up Windows' high-contrast colours the way the stock ones would — honouring the
    /// setting with a matching theme is the closest equivalent.
    /// </summary>
    private static string DefaultForThisMachine() =>
        SystemParameters.HighContrast ? ThemeCatalog.HighContrastDark.Id : ThemeCatalog.Default.Id;

    public void ReloadAvailableThemes()
    {
        var user = _store.Load(out _storeIssues);
        _available = ThemeCatalog.BuiltIns.Concat(user).ToList();
    }

    public void SelectTheme(string themeId)
    {
        if (Find(themeId) is not { } definition) return;

        // Unsaved edits belong to the theme they were made against, so drop them rather than
        // carrying them across; anything already persisted is reloaded per theme below.
        LoadOverrides(themeId);
        Apply(definition);

        _settings.ThemeId = themeId;
        _settings.Save();
    }

    public ThemeColor GetColor(string token) => Current[token];

    public bool IsOverridden(string token) => _overrides.ContainsKey(token);

    public ThemeColor GetThemeColor(string token) =>
        ThemeResolver.Resolve(_definition, Find)[token];

    public void SetOverride(string token, ThemeColor? color)
    {
        if (!ThemeToken.IsKnown(token)) return;

        if (color is { } value) _overrides[token] = value.ToHex();
        else _overrides.Remove(token);

        Apply(_definition);
    }

    public void PersistOverrides()
    {
        var pruned = ThemeResolver.PruneOverrides(_overrides, ThemeResolver.Resolve(_definition, Find));
        _overrides = pruned;
        _persistedOverrides = new Dictionary<string, string>(pruned, StringComparer.Ordinal);

        if (pruned.Count == 0) _settings.ThemeOverrides.Remove(_definition.Id);
        else _settings.ThemeOverrides[_definition.Id] = pruned;

        _settings.Save();
        Apply(_definition);
    }

    public void RevertOverrides()
    {
        _overrides = new Dictionary<string, string>(_persistedOverrides, StringComparer.Ordinal);
        Apply(_definition);
    }

    public void ResetAllOverrides()
    {
        _overrides.Clear();
        PersistOverrides();
    }

    public bool TrySaveAsNewTheme(string name, out ThemeDefinition? created, out string? error)
    {
        created = null;

        if (string.IsNullOrWhiteSpace(name))
        {
            error = "Give the theme a name.";
            return false;
        }

        var id = ThemeId.Unique(name, _available.Select(t => t.Id));

        // Baked flat rather than left as a diff against the theme it started from: a saved theme
        // should not change because a future release retunes Dark+.
        var colors = ThemeToken.All.ToDictionary(key => key, key => Current[key].ToHex(), StringComparer.Ordinal);
        var definition = new ThemeDefinition
        {
            Id = id,
            Name = name.Trim(),
            IsDark = Current.IsDark,
            Colors = colors,
        };

        if (!_store.TrySave(definition, ThemeJson.Serialize(definition), out error)) return false;

        ReloadAvailableThemes();
        _overrides.Clear();
        _persistedOverrides.Clear();
        _settings.ThemeOverrides.Remove(id);
        Apply(definition);
        _settings.ThemeId = id;
        _settings.Save();

        created = definition;
        return true;
    }

    public bool TryImport(string sourcePath, out ThemeDefinition? imported, out string? error)
    {
        imported = null;
        try
        {
            if (!ThemeJson.TryDeserialize(File.ReadAllText(sourcePath), out var parsed, out error))
                return false;

            // Rename rather than overwrite: importing must never silently replace a theme the user
            // already has, and a built-in id would shadow something we ship. The id also becomes a
            // filename, and this one came out of a file someone else wrote — so keeping it is
            // conditional on ThemeId.IsSafe, or an id like "C:\Windows\Temp\evil" would land there
            // instead of in the themes folder.
            var keepId = ThemeId.IsSafe(parsed!.Id) &&
                ThemeCatalog.Find(parsed.Id) is null &&
                !_available.Any(t => string.Equals(t.Id, parsed.Id, StringComparison.OrdinalIgnoreCase));

            var id = keepId ? parsed.Id : ThemeId.Unique(parsed.Name, _available.Select(t => t.Id));

            var definition = new ThemeDefinition
            {
                Id = id,
                Name = parsed.Name,
                BaseThemeId = parsed.BaseThemeId,
                IsDark = parsed.IsDark,
                Colors = parsed.Colors,
            };

            if (!_store.TrySave(definition, ThemeJson.Serialize(definition), out error)) return false;

            ReloadAvailableThemes();
            imported = definition;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            error = ex.Message;
            return false;
        }
    }

    public bool TryExport(string destinationPath, out string? error)
    {
        try
        {
            File.WriteAllText(destinationPath, ThemeJson.SerializeResolved(Current));
            error = null;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            error = ex.Message;
            return false;
        }
    }

    public bool TryDeleteUserTheme(string themeId, out string? error)
    {
        if (ThemeCatalog.Find(themeId) is not null)
        {
            error = "Built-in themes can't be deleted.";
            return false;
        }

        if (!_store.TryDelete(themeId, out error)) return false;

        _settings.ThemeOverrides.Remove(themeId);
        ReloadAvailableThemes();

        if (string.Equals(_definition.Id, themeId, StringComparison.OrdinalIgnoreCase))
            SelectTheme(ThemeCatalog.Default.Id);
        else
            _settings.Save();

        return true;
    }

    private void Apply(ThemeDefinition definition)
    {
        _definition = definition;
        Current = ThemeResolver.Resolve(definition, Find, _overrides);
        Issues = _storeIssues.Concat(Current.Issues).ToList();

        ThemeTokenDictionary.ApplyTheme(Current);
        ThemeChanged?.Invoke(this, EventArgs.Empty);
    }

    private ThemeDefinition? Find(string id) =>
        _available.FirstOrDefault(t => string.Equals(t.Id, id, StringComparison.OrdinalIgnoreCase));

    /// <summary>
    /// A selected theme can legitimately be absent for one launch — a file still syncing, a themes
    /// folder on a drive that isn't mounted — so fall back for the session and say so, but leave
    /// <see cref="AppSettings.ThemeId"/> alone. Rewriting it would destroy the choice permanently.
    /// </summary>
    private ThemeDefinition FindOrFallBack(string id)
    {
        if (Find(id) is { } found) return found;

        _storeIssues = _storeIssues.Append(new ThemeIssue(ThemeIssueSeverity.Error, null,
            $"Theme '{id}' was not found; using {ThemeCatalog.Default.Name} for now.")).ToList();
        return ThemeCatalog.Default;
    }

    private void LoadOverrides(string themeId)
    {
        _persistedOverrides = _settings.ThemeOverrides.TryGetValue(themeId, out var stored)
            ? new Dictionary<string, string>(stored, StringComparer.Ordinal)
            : new Dictionary<string, string>(StringComparer.Ordinal);
        _overrides = new Dictionary<string, string>(_persistedOverrides, StringComparer.Ordinal);
    }
}
