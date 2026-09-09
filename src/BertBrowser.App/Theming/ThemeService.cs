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
    private readonly ISystemAppearance _appearance;

    private List<ThemeDefinition> _available = new(ThemeCatalog.BuiltIns);
    private IReadOnlyList<ThemeIssue> _storeIssues = Array.Empty<ThemeIssue>();
    private IReadOnlyList<ThemeIssue> _followIssues = Array.Empty<ThemeIssue>();
    private Dictionary<string, string> _overrides = new(StringComparer.Ordinal);
    private Dictionary<string, string> _persistedOverrides = new(StringComparer.Ordinal);
    private ThemeDefinition _definition = ThemeCatalog.Default;
    private bool _following;
    private SystemAppearance _appearanceState;

    /// <summary>
    /// Unsaved colour edits, parked by theme id for the length of the session.
    /// </summary>
    /// <remarks>
    /// A system-driven switch is not a decision anyone made: Windows can flip at sunset while the
    /// theme editor is open, and dropping half an hour of unsaved edits on the floor for that would
    /// be indefensible. So the <em>system</em> path parks them and puts them back if it switches
    /// back. <see cref="SelectTheme"/> is unchanged — deliberately choosing another theme still
    /// discards them, which is what its own comment says and what Revert is for.
    /// </remarks>
    private readonly Dictionary<string, Dictionary<string, string>> _parkedOverrides =
        new(StringComparer.OrdinalIgnoreCase);

    public ThemeService(AppSettings settings, UserThemeStore store, ISystemAppearance appearance)
    {
        _settings = settings;
        _store = store;
        _appearance = appearance;
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

        _following = SystemThemeRules.ShouldFollow(
            _settings.FollowSystemTheme, _settings.LoadedFromDisk);
        if (_settings.FollowSystemTheme is null)
        {
            // Pinned now, or launch two would find a settings.json and read the null as "no" — a
            // fresh install would follow for exactly one session.
            _settings.FollowSystemTheme = _following;
            _settings.Save();
        }

        // Subscribed whichever mode we are in: turning following on from Settings must not need a
        // restart, and DefaultForThisMachine has to stay right if it is.
        _appearanceState = _appearance.Current;
        _appearance.Changed += OnSystemAppearanceChanged;

        ApplyForCurrentState();
    }

    /// <summary>Resolves what should be showing right now — from Windows if following, from the
    /// user's pick if not — and applies it. Persists nothing.</summary>
    private void ApplyForCurrentState()
    {
        string id;
        if (_following)
        {
            var choice = SystemThemeRules.Decide(
                _appearanceState, _settings.LightThemeId, _settings.DarkThemeId, Find);
            _followIssues = choice.Issue is { } issue ? new[] { issue } : Array.Empty<ThemeIssue>();
            id = choice.ThemeId;
        }
        else
        {
            _followIssues = Array.Empty<ThemeIssue>();
            id = _settings.ThemeId ?? DefaultForThisMachine();
        }

        LoadOverrides(id);
        Apply(FindOrFallBack(id));
    }

    /// <summary>
    /// What to use when not following and the user has never chosen. Our control templates are
    /// fully custom, so they do not pick up Windows' high-contrast colours the way the stock ones
    /// would — honouring the setting with a matching theme is the closest equivalent.
    /// </summary>
    /// <remarks>The high-contrast read goes through <see cref="ISystemAppearance"/> rather than
    /// <c>SystemParameters</c> directly, so a scripted run cannot inherit the setting of whichever
    /// machine took the screenshots.</remarks>
    private string DefaultForThisMachine() =>
        _appearanceState.IsHighContrast ? ThemeCatalog.HighContrastDark.Id : ThemeCatalog.Default.Id;

    private void OnSystemAppearanceChanged(object? sender, EventArgs e)
    {
        _appearanceState = _appearance.Current;

        // Read even when not following, so DefaultForThisMachine is right if following is turned on
        // later this session — but nothing is applied, because an unfollowed theme is the user's.
        if (!_following) return;

        var choice = SystemThemeRules.Decide(
            _appearanceState, _settings.LightThemeId, _settings.DarkThemeId, Find);
        _followIssues = choice.Issue is { } issue ? new[] { issue } : Array.Empty<ThemeIssue>();

        // Dark+ in both slots, or high contrast toggled while already on the accessibility theme:
        // repainting would be a no-op that still fires ThemeChanged at every window and re-reads
        // the override table for nothing.
        if (string.Equals(choice.ThemeId, _definition.Id, StringComparison.OrdinalIgnoreCase)) return;

        ParkUnsavedOverrides();
        LoadOverrides(choice.ThemeId);
        Apply(FindOrFallBack(choice.ThemeId));

        // Note what is deliberately absent: no _settings.ThemeId assignment and no Save(). Nothing
        // about the OS moving belongs in the file — writing it would quietly convert someone who is
        // following into someone who is pinned, at the next sunset.
    }

    private void ParkUnsavedOverrides()
    {
        if (_overrides.Count == _persistedOverrides.Count &&
            !_overrides.Except(_persistedOverrides).Any())
        {
            return;
        }

        _parkedOverrides[_definition.Id] =
            new Dictionary<string, string>(_overrides, StringComparer.Ordinal);
    }

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

        // Choosing one theme by hand is the end of following: the two pickers that stay visible in
        // follow mode go through SetSlotTheme, so anything reaching here is someone naming the one
        // theme they want to look at.
        _following = false;
        _followIssues = Array.Empty<ThemeIssue>();
        _settings.FollowSystemTheme = false;
        _settings.ThemeId = themeId;
        _settings.Save();
    }

    public bool IsFollowingSystem => _following;

    public string LightThemeId => _settings.LightThemeId ?? SystemThemeRules.DefaultLightThemeId;

    public string DarkThemeId => _settings.DarkThemeId ?? SystemThemeRules.DefaultDarkThemeId;

    public void SetFollowSystem(bool follow)
    {
        if (_following == follow) return;

        // Unticking keeps what is on screen: the checkbox says "stop tracking Windows", not "go
        // back to whatever was selected last year". ThemeId was left alone while following exactly
        // so that ticking it on and off again is lossless in both directions.
        if (!follow) _settings.ThemeId = _definition.Id;

        _following = follow;
        _settings.FollowSystemTheme = follow;
        _settings.Save();

        _appearanceState = _appearance.Current;
        ApplyForCurrentState();
    }

    public void SetSlotTheme(bool dark, string themeId)
    {
        if (Find(themeId) is null) return;

        if (dark) _settings.DarkThemeId = themeId;
        else _settings.LightThemeId = themeId;
        _settings.Save();

        // Applied only when it is the slot Windows is currently asking for: setting the light theme
        // while Windows is dark configures, it does not preview. Showing it instead would leave the
        // app light on a dark desktop until the next OS event yanked it away, which reads as the
        // setting being lost.
        if (_following) ApplyForCurrentState();
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

        if (_following)
        {
            // Baked from whatever is on screen, so it belongs to the slot that produced it — and,
            // being that slot, it is already applied and nothing flips. Writing ThemeId here
            // instead would persist an id nothing reads and let the next OS event throw the
            // brand-new theme away.
            if (definition.IsDark) _settings.DarkThemeId = id;
            else _settings.LightThemeId = id;
        }
        else
        {
            _settings.ThemeId = id;
        }

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
        _parkedOverrides.Remove(themeId);

        // A slot pointing at a theme just deleted is different from one pointing at a theme merely
        // absent (a file still syncing): we know this one is gone, because we removed it. So this
        // one is cleared, while SystemThemeRules' "leave the setting alone" protects a choice that
        // might still come back.
        if (string.Equals(_settings.LightThemeId, themeId, StringComparison.OrdinalIgnoreCase))
            _settings.LightThemeId = null;
        if (string.Equals(_settings.DarkThemeId, themeId, StringComparison.OrdinalIgnoreCase))
            _settings.DarkThemeId = null;

        ReloadAvailableThemes();

        if (_following)
        {
            // Falls back to Light+/Dark+ without SelectTheme, which would stop following.
            _settings.Save();
            ApplyForCurrentState();
        }
        else if (string.Equals(_definition.Id, themeId, StringComparison.OrdinalIgnoreCase))
        {
            SelectTheme(ThemeCatalog.Default.Id);
        }
        else
        {
            _settings.Save();
        }

        return true;
    }

    private void Apply(ThemeDefinition definition)
    {
        _definition = definition;
        Current = ThemeResolver.Resolve(definition, Find, _overrides);
        Issues = _storeIssues.Concat(_followIssues).Concat(Current.Issues).ToList();

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

        // Edits parked by a system-driven switch (see ParkUnsavedOverrides) outrank what was
        // persisted: Windows flipping at sunset must not cost the user the colours they were in the
        // middle of choosing.
        if (_parkedOverrides.Remove(themeId, out var parked)) _overrides = parked;
    }
}
