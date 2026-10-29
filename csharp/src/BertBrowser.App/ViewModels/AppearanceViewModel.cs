using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Windows.Data;
using System.Windows.Media;
using BertBrowser.App.Theming;
using BertBrowser.Core.Theming;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>One editable colour in the theme editor.</summary>
public sealed partial class ThemeTokenViewModel : ObservableObject
{
    private readonly IThemeService _theme;
    private bool _updating;

    public ThemeTokenViewModel(ThemeTokenDescriptor descriptor, IThemeService theme)
    {
        Descriptor = descriptor;
        _theme = theme;
        Refresh();
    }

    public ThemeTokenDescriptor Descriptor { get; }

    public string Key => Descriptor.Key;
    public string Group => Descriptor.Group;
    public string DisplayName => Descriptor.DisplayName;

    public string ToolTipText => string.IsNullOrEmpty(Descriptor.Description)
        ? Descriptor.Key
        : $"{Descriptor.Description}\n{Descriptor.Key}";

    [ObservableProperty]
    private Color _color;

    [ObservableProperty]
    private string _hex = "";

    [ObservableProperty]
    private bool _isOverridden;

    [ObservableProperty]
    private bool _isHexValid = true;

    /// <summary>Re-reads the live theme. Called after a theme switch or a reset.</summary>
    public void Refresh()
    {
        _updating = true;
        var value = _theme.GetColor(Key);
        Color = ThemeTokenDictionary.ToMediaColor(value);
        Hex = value.ToHex();
        IsHexValid = true;
        IsOverridden = _theme.IsOverridden(Key);
        _updating = false;
    }

    partial void OnColorChanged(Color value)
    {
        if (_updating) return;
        _updating = true;
        Hex = ThemeTokenDictionary.ToThemeColor(value).ToHex();
        IsHexValid = true;
        _updating = false;
        Apply(ThemeTokenDictionary.ToThemeColor(value));
    }

    partial void OnHexChanged(string value)
    {
        if (_updating) return;

        if (!ThemeColor.TryParse(value, out var parsed))
        {
            // Typing "#1E1E" on the way to "#1E1E1E" is not an error worth acting on; the field
            // just marks itself invalid until the text is a colour again.
            IsHexValid = false;
            return;
        }

        _updating = true;
        IsHexValid = true;
        Color = ThemeTokenDictionary.ToMediaColor(parsed);
        _updating = false;
        Apply(parsed);
    }

    private void Apply(ThemeColor value)
    {
        _theme.SetOverride(Key, value);
        IsOverridden = _theme.IsOverridden(Key);
    }

    public void Reset()
    {
        _theme.SetOverride(Key, null);
        Refresh();
    }
}

/// <summary>
/// Backs the theme picker and the colour editor. Changes apply to the running app immediately —
/// there is no point previewing a theme you cannot see — so this deliberately does not follow the
/// copy-then-commit contract the rest of the Settings dialog uses.
/// </summary>
public sealed partial class AppearanceViewModel : ObservableObject
{
    private readonly IThemeService _theme;
    private readonly List<ThemeTokenViewModel> _allTokens;
    private bool _switching;

    public AppearanceViewModel(IThemeService theme)
    {
        _theme = theme;
        _allTokens = ThemeToken.Descriptors.Select(d => new ThemeTokenViewModel(d, theme)).ToList();

        Themes = new ObservableCollection<ThemeDefinition>(theme.Available);
        _selectedTheme = theme.Available.FirstOrDefault(t =>
            string.Equals(t.Id, theme.Current.Id, StringComparison.OrdinalIgnoreCase));

        LightThemes = new ObservableCollection<ThemeDefinition>(
            SystemThemeRules.SlotChoices(theme.Available, dark: false, theme.LightThemeId));
        DarkThemes = new ObservableCollection<ThemeDefinition>(
            SystemThemeRules.SlotChoices(theme.Available, dark: true, theme.DarkThemeId));

        _followSystemTheme = theme.IsFollowingSystem;
        _selectedLightTheme = Pick(LightThemes, theme.LightThemeId);
        _selectedDarkTheme = Pick(DarkThemes, theme.DarkThemeId);

        Tokens = new ObservableCollection<ThemeTokenViewModel>();
        TokensView = new CollectionViewSource { Source = Tokens };
        TokensView.GroupDescriptions.Add(new PropertyGroupDescription(nameof(ThemeTokenViewModel.Group)));

        RefreshTokenList();
    }

    public ObservableCollection<ThemeDefinition> Themes { get; }

    /// <summary>What the two slot pickers offer while following — the light and dark halves of
    /// <see cref="Themes"/>, less the accessibility theme, which arrives on its own switch.</summary>
    public ObservableCollection<ThemeDefinition> LightThemes { get; }

    public ObservableCollection<ThemeDefinition> DarkThemes { get; }

    public ObservableCollection<ThemeTokenViewModel> Tokens { get; }

    public CollectionViewSource TokensView { get; }

    [ObservableProperty]
    private ThemeDefinition? _selectedTheme;

    /// <summary>Whether the theme tracks the Windows light/dark and high-contrast settings.</summary>
    [ObservableProperty]
    private bool _followSystemTheme;

    [ObservableProperty]
    private ThemeDefinition? _selectedLightTheme;

    [ObservableProperty]
    private ThemeDefinition? _selectedDarkTheme;

    [ObservableProperty]
    private string _searchText = "";

    [ObservableProperty]
    private bool _showAllColors;

    [ObservableProperty]
    private string? _statusMessage;

    /// <summary>Problems with the current selection, for the notice line in Settings.</summary>
    public string? IssueText => _theme.Issues.Count == 0
        ? null
        : string.Join(" ", _theme.Issues.Select(i => i.Message));

    public bool CanDeleteSelectedTheme => SelectedTheme is { IsBuiltIn: false };

    partial void OnSelectedThemeChanged(ThemeDefinition? value)
    {
        if (_switching || value is null) return;

        if (_theme.IsFollowingSystem)
        {
            // Reached from the theme editor's own picker, which stays visible in both modes. While
            // following, naming a theme is a choice of what to look at when Windows is in that
            // mode, not what to look at now — so it assigns the matching slot and following
            // survives. Applying it instead would leave a light theme up on a dark desktop until
            // the next OS event took it away, which reads as the setting being lost.
            _theme.SetSlotTheme(value.IsDark, value.Id);
            SyncSlotSelection();
            StatusMessage = value.IsDark == _theme.Current.IsDark
                ? $"\"{value.Name}\" is now your {Side(value.IsDark)} theme."
                : $"\"{value.Name}\" is now your {Side(value.IsDark)} theme. Windows is in " +
                  $"{Side(!value.IsDark)} mode, so {_theme.Current.Name} stays in use.";
        }
        else
        {
            _theme.SelectTheme(value.Id);
        }

        foreach (var token in _allTokens) token.Refresh();
        OnPropertyChanged(nameof(IssueText));
        OnPropertyChanged(nameof(CanDeleteSelectedTheme));
    }

    private static string Side(bool dark) => dark ? "dark" : "light";

    partial void OnFollowSystemThemeChanged(bool value)
    {
        if (_switching) return;

        _theme.SetFollowSystem(value);
        SyncAfterExternalChange();
    }

    partial void OnSelectedLightThemeChanged(ThemeDefinition? value)
    {
        if (_switching || value is null) return;

        _theme.SetSlotTheme(dark: false, value.Id);
        SyncAfterExternalChange();
    }

    partial void OnSelectedDarkThemeChanged(ThemeDefinition? value)
    {
        if (_switching || value is null) return;

        _theme.SetSlotTheme(dark: true, value.Id);
        SyncAfterExternalChange();
    }

    /// <summary>
    /// The theme changed underneath us — Windows flipped at sunset, or a slot was reassigned.
    /// </summary>
    /// <remarks>
    /// The <c>_switching</c> guard is the whole point: without it, writing a selection here is
    /// indistinguishable from the user picking one, so <see cref="SelectTheme"/> would run and
    /// following would silently stop. The bug that produces looks like "matching Windows keeps
    /// turning itself off", and only when Settings happened to be open.
    /// </remarks>
    public void SyncAfterExternalChange()
    {
        _switching = true;
        FollowSystemTheme = _theme.IsFollowingSystem;
        SelectedTheme = Themes.FirstOrDefault(t =>
            string.Equals(t.Id, _theme.Current.Id, StringComparison.OrdinalIgnoreCase));
        SelectedLightTheme = Pick(LightThemes, _theme.LightThemeId);
        SelectedDarkTheme = Pick(DarkThemes, _theme.DarkThemeId);
        _switching = false;

        foreach (var token in _allTokens) token.Refresh();
        OnPropertyChanged(nameof(IssueText));
        OnPropertyChanged(nameof(CanDeleteSelectedTheme));
    }

    /// <summary>Moves the two slot selections onto what the service now holds, without letting that
    /// read as a fresh choice.</summary>
    private void SyncSlotSelection()
    {
        _switching = true;
        SelectedLightTheme = Pick(LightThemes, _theme.LightThemeId);
        SelectedDarkTheme = Pick(DarkThemes, _theme.DarkThemeId);
        _switching = false;
    }

    private static void Refill(
        ObservableCollection<ThemeDefinition> target, IReadOnlyList<ThemeDefinition> source)
    {
        target.Clear();
        foreach (var theme in source) target.Add(theme);
    }

    private static ThemeDefinition? Pick(IEnumerable<ThemeDefinition> from, string id) =>
        from.FirstOrDefault(t => string.Equals(t.Id, id, StringComparison.OrdinalIgnoreCase));

    partial void OnSearchTextChanged(string value) => RefreshTokenList();

    partial void OnShowAllColorsChanged(bool value) => RefreshTokenList();

    private void RefreshTokenList()
    {
        var needle = SearchText.Trim();

        Tokens.Clear();
        foreach (var token in _allTokens)
        {
            if (!ShowAllColors && !token.Descriptor.IsCore && needle.Length == 0) continue;
            if (needle.Length > 0 &&
                !token.DisplayName.Contains(needle, StringComparison.OrdinalIgnoreCase) &&
                !token.Key.Contains(needle, StringComparison.OrdinalIgnoreCase) &&
                !token.Group.Contains(needle, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }
            Tokens.Add(token);
        }
    }

    [RelayCommand]
    private void ResetToken(ThemeTokenViewModel? token)
    {
        token?.Reset();
        StatusMessage = token is null ? null : $"{token.DisplayName} reset.";
    }

    [RelayCommand]
    private void ResetAll()
    {
        _theme.ResetAllOverrides();
        foreach (var token in _allTokens) token.Refresh();
        StatusMessage = "All colours reset to the theme's defaults.";
    }

    [RelayCommand]
    private void Revert()
    {
        _theme.RevertOverrides();
        foreach (var token in _allTokens) token.Refresh();
        StatusMessage = "Unsaved colour changes discarded.";
    }

    [RelayCommand]
    private void Keep()
    {
        _theme.PersistOverrides();
        foreach (var token in _allTokens) token.Refresh();
        StatusMessage = "Colours saved.";
    }

    public bool TrySaveAsNewTheme(string name, out string? error)
    {
        if (!_theme.TrySaveAsNewTheme(name, out var created, out error)) return false;

        ReloadThemes(created?.Id);
        StatusMessage = $"Saved as \"{created?.Name}\".";
        return true;
    }

    public bool TryImport(string path, out string? error)
    {
        if (!_theme.TryImport(path, out var imported, out error)) return false;

        ReloadThemes(imported?.Id);
        StatusMessage = $"Imported \"{imported?.Name}\".";
        return true;
    }

    public bool TryExport(string path, out string? error)
    {
        if (!_theme.TryExport(path, out error)) return false;
        StatusMessage = "Theme exported.";
        return true;
    }

    public bool TryDeleteSelected(out string? error)
    {
        error = null;
        if (SelectedTheme is not { IsBuiltIn: false } target) return false;
        if (!_theme.TryDeleteUserTheme(target.Id, out error)) return false;

        ReloadThemes(_theme.Current.Id);
        StatusMessage = $"Deleted \"{target.Name}\".";
        return true;
    }

    private void ReloadThemes(string? selectId)
    {
        _theme.ReloadAvailableThemes();

        // The selection is set without re-entering SelectTheme: the service has already applied
        // whatever we are about to show as selected.
        _switching = true;
        Themes.Clear();
        foreach (var theme in _theme.Available) Themes.Add(theme);

        Refill(LightThemes, SystemThemeRules.SlotChoices(_theme.Available, false, _theme.LightThemeId));
        Refill(DarkThemes, SystemThemeRules.SlotChoices(_theme.Available, true, _theme.DarkThemeId));

        SelectedTheme = Themes.FirstOrDefault(t =>
            string.Equals(t.Id, selectId ?? _theme.Current.Id, StringComparison.OrdinalIgnoreCase));
        FollowSystemTheme = _theme.IsFollowingSystem;
        SelectedLightTheme = Pick(LightThemes, _theme.LightThemeId);
        SelectedDarkTheme = Pick(DarkThemes, _theme.DarkThemeId);
        _switching = false;

        foreach (var token in _allTokens) token.Refresh();
        OnPropertyChanged(nameof(IssueText));
        OnPropertyChanged(nameof(CanDeleteSelectedTheme));
    }
}
