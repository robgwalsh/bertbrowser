using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.Core.Models;

namespace BertBrowser.App.ViewModels;

/// <summary>Editable row in the custom-commands list.</summary>
public sealed partial class CustomCommandItemViewModel : ObservableObject
{
    [ObservableProperty]
    private string _name = "";

    [ObservableProperty]
    private string _command = "";

    [ObservableProperty]
    private string _arguments = "";

    [ObservableProperty]
    private bool _appliesToFiles = true;

    [ObservableProperty]
    private bool _appliesToDirectories;

    [ObservableProperty]
    private bool _runElevated;

    public CustomCommandItemViewModel()
    {
    }

    public CustomCommandItemViewModel(CustomCommandDefinition definition)
    {
        Name = definition.Name;
        Command = definition.Command;
        Arguments = definition.Arguments;
        AppliesToFiles = definition.AppliesToFiles;
        AppliesToDirectories = definition.AppliesToDirectories;
        RunElevated = definition.RunElevated;
    }

    public CustomCommandDefinition ToDefinition() => new()
    {
        Name = Name.Trim(),
        Command = Command.Trim(),
        Arguments = Arguments.Trim(),
        AppliesToFiles = AppliesToFiles,
        AppliesToDirectories = AppliesToDirectories,
        RunElevated = RunElevated,
    };
}

/// <summary>A page of the Settings dialog, i.e. one entry in its left-hand navigation list.</summary>
public enum SettingsCategory
{
    General,
    Appearance,
    Commands,
}

/// <summary>One row of the navigation list. <see cref="Glyph"/> is a Segoe Fluent Icons codepoint.</summary>
public sealed class SettingsCategoryViewModel
{
    public SettingsCategoryViewModel(SettingsCategory id, string name, string glyph)
    {
        Id = id;
        Name = name;
        Glyph = glyph;
    }

    public SettingsCategory Id { get; }

    public string Name { get; }

    public string Glyph { get; }
}

public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly AppSettings _settings;

    public ObservableCollection<CustomCommandItemViewModel> Commands { get; }

    /// <summary>The navigation list on the left; exactly one page is shown at a time.</summary>
    public IReadOnlyList<SettingsCategoryViewModel> Categories { get; }

    [ObservableProperty]
    private SettingsCategoryViewModel _selectedCategory;

    [ObservableProperty]
    private CustomCommandItemViewModel? _selectedCommand;

    [ObservableProperty]
    private bool _showHiddenItems;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ScrollSpeedText))]
    private double _scrollSpeed;

    public string ScrollSpeedText => $"{ScrollSpeed:0.0}×";

    /// <summary>Tile shapes offered by the picker. Seeded from <see cref="AspectRatio.Presets"/>,
    /// plus whatever the settings file already holds if it isn't one of them — otherwise a ratio
    /// typed in by hand would be silently replaced the first time this dialog is saved.</summary>
    public IReadOnlyList<AspectRatio> TileAspectOptions { get; }

    [ObservableProperty]
    private AspectRatio _tileAspect;

    /// <summary>
    /// Theme selection and editing. Unlike everything else here it applies live rather than on
    /// Save — see the note in the dialog.
    /// </summary>
    public AppearanceViewModel Appearance { get; }

    public SettingsViewModel(AppSettings settings, IThemeService theme)
    {
        Categories = new[]
        {
            new SettingsCategoryViewModel(SettingsCategory.General, "General", "\uE713"),
            new SettingsCategoryViewModel(SettingsCategory.Appearance, "Appearance", "\uE790"),
            new SettingsCategoryViewModel(SettingsCategory.Commands, "Commands", "\uE8A7"),
        };
        _selectedCategory = Categories[0];

        Appearance = new AppearanceViewModel(theme);
        _settings = settings;
        ShowHiddenItems = settings.ShowHiddenItems;
        ScrollSpeed = settings.ScrollSpeedMultiplier;

        TileAspect = AspectRatio.Parse(settings.TileAspectRatio);
        TileAspectOptions = AspectRatio.Presets.Contains(TileAspect)
            ? AspectRatio.Presets
            : AspectRatio.Presets.Append(TileAspect).ToList();

        Commands = new ObservableCollection<CustomCommandItemViewModel>(
            settings.CustomCommands.Select(d => new CustomCommandItemViewModel(d)));
        SelectedCommand = Commands.FirstOrDefault();
    }

    [RelayCommand]
    private void Add()
    {
        var item = new CustomCommandItemViewModel { Name = "New command" };
        Commands.Add(item);
        SelectedCommand = item;
    }

    [RelayCommand]
    private void Remove()
    {
        if (SelectedCommand is not { } selected) return;
        var index = Commands.IndexOf(selected);
        Commands.Remove(selected);
        SelectedCommand = Commands.Count > 0 ? Commands[Math.Min(index, Commands.Count - 1)] : null;
    }

    /// <summary>Brings a page to the front — the only way to point at something now that the
    /// dialog shows one category at a time.</summary>
    public void ShowCategory(SettingsCategory category)
    {
        SelectedCategory = Categories.First(c => c.Id == category);
    }

    /// <summary>Validates and persists all commands to settings.json.</summary>
    public bool TrySave(out string? error)
    {
        foreach (var command in Commands)
        {
            string? problem = null;
            if (string.IsNullOrWhiteSpace(command.Name) || string.IsNullOrWhiteSpace(command.Command))
                problem = "Every command needs a name and a program.";
            else if (!command.AppliesToFiles && !command.AppliesToDirectories)
                problem = $"'{command.Name}' must apply to files, folders, or both.";

            if (problem is not null)
            {
                // The offending command may be on a page the user cannot see, so go there first —
                // otherwise the message names a field that is nowhere on screen.
                SelectedCommand = command;
                ShowCategory(SettingsCategory.Commands);
                error = problem;
                return false;
            }
        }

        _settings.CustomCommands = Commands.Select(c => c.ToDefinition()).ToList();
        _settings.ShowHiddenItems = ShowHiddenItems;
        _settings.ScrollSpeedMultiplier = ScrollSpeed;
        _settings.TileAspectRatio = TileAspect.ToString();
        _settings.Save();
        error = null;
        return true;
    }
}
