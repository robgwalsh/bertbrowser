using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;

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

/// <summary>Editable row in the "New" submenu's file-type list.</summary>
public sealed partial class NewFileTypeItemViewModel : ObservableObject
{
    [ObservableProperty]
    private string _label = "";

    [ObservableProperty]
    private string _extension = "";

    /// <summary>A file to copy contents from, or empty for a new file that starts empty.</summary>
    [ObservableProperty]
    private string _templatePath = "";

    [ObservableProperty]
    private bool _enabled = true;

    public NewFileTypeItemViewModel()
    {
    }

    public NewFileTypeItemViewModel(NewFileTemplate template)
    {
        Label = template.Label;
        Extension = template.Extension;
        TemplatePath = template.TemplatePath ?? "";
        Enabled = template.Enabled;
    }

    public NewFileTemplate ToTemplate() => new()
    {
        Label = Label.Trim(),
        Extension = Extension.Trim(),
        TemplatePath = TemplatePath.Trim() is { Length: > 0 } path ? path : null,
        Enabled = Enabled,
    };
}

/// <summary>A page of the Settings dialog, i.e. one entry in its left-hand navigation list.</summary>
public enum SettingsCategory
{
    General,
    Appearance,
    NewItems,
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
    private readonly IShellNewCatalog? _shellNew;

    public ObservableCollection<CustomCommandItemViewModel> Commands { get; }

    /// <summary>The "New" submenu file types, in menu order.</summary>
    public ObservableCollection<NewFileTypeItemViewModel> NewFileTypes { get; }

    /// <summary>The navigation list on the left; exactly one page is shown at a time.</summary>
    public IReadOnlyList<SettingsCategoryViewModel> Categories { get; }

    [ObservableProperty]
    private SettingsCategoryViewModel _selectedCategory;

    [ObservableProperty]
    private CustomCommandItemViewModel? _selectedCommand;

    [ObservableProperty]
    private NewFileTypeItemViewModel? _selectedNewFileType;

    [ObservableProperty]
    private bool _isImporting;

    [ObservableProperty]
    private string _importStatus = "";

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

    public SettingsViewModel(
        AppSettings settings, IThemeService theme, IShellNewCatalog? shellNew = null)
    {
        Categories = new[]
        {
            new SettingsCategoryViewModel(SettingsCategory.General, "General", "\uE713"),
            new SettingsCategoryViewModel(SettingsCategory.Appearance, "Appearance", "\uE790"),
            new SettingsCategoryViewModel(SettingsCategory.NewItems, "New items", "\uE710"),
            new SettingsCategoryViewModel(SettingsCategory.Commands, "Commands", "\uE8A7"),
        };
        _selectedCategory = Categories[0];

        Appearance = new AppearanceViewModel(theme);
        _shellNew = shellNew;
        _settings = settings;

        // Null means never configured, which is what ships the defaults; an empty list means the
        // user removed them all and must stay empty.
        NewFileTypes = new ObservableCollection<NewFileTypeItemViewModel>(
            (settings.NewFileTypes ?? NewFileTemplate.Defaults())
                .Select(t => new NewFileTypeItemViewModel(t)));
        SelectedNewFileType = NewFileTypes.FirstOrDefault();
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

    // --- New-item types ---

    [RelayCommand]
    private void AddNewFileType()
    {
        var item = new NewFileTypeItemViewModel { Label = "New type", Extension = ".txt" };
        NewFileTypes.Add(item);
        SelectedNewFileType = item;
    }

    [RelayCommand]
    private void RemoveNewFileType()
    {
        if (SelectedNewFileType is not { } selected) return;
        var index = NewFileTypes.IndexOf(selected);
        NewFileTypes.Remove(selected);
        SelectedNewFileType =
            NewFileTypes.Count > 0 ? NewFileTypes[Math.Min(index, NewFileTypes.Count - 1)] : null;
    }

    /// <summary>The list is the menu's order, which is the point of owning it rather than reading
    /// the registry live.</summary>
    [RelayCommand]
    private void MoveNewFileTypeUp() => MoveNewFileType(-1);

    [RelayCommand]
    private void MoveNewFileTypeDown() => MoveNewFileType(1);

    private void MoveNewFileType(int delta)
    {
        if (SelectedNewFileType is not { } selected) return;
        var index = NewFileTypes.IndexOf(selected);
        var target = index + delta;
        if (target < 0 || target >= NewFileTypes.Count) return;

        NewFileTypes.Move(index, target);
        SelectedNewFileType = selected;
    }

    /// <summary>Adds the types Windows knows about that aren't listed yet. Reads the registry and
    /// never writes to it, so Explorer's own New menu is untouched; entries already here keep their
    /// place and their settings, so this is safe to press twice.</summary>
    [RelayCommand]
    private async Task ImportFromWindowsAsync()
    {
        if (_shellNew is null || IsImporting) return;

        IsImporting = true;
        try
        {
            var discovered = await _shellNew.ReadAsync();
            var existing = NewFileTypes.Select(t => t.ToTemplate()).ToList();
            var merged = ShellNewImport.Merge(existing, discovered);

            var added = merged.Count - existing.Count;
            foreach (var template in merged.Skip(existing.Count))
                NewFileTypes.Add(new NewFileTypeItemViewModel(template));

            ImportStatus = added == 0
                ? "Nothing new — every type Windows offers is already listed."
                : $"Added {added:N0} type(s) from Windows.";
        }
        finally
        {
            IsImporting = false;
        }
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
        foreach (var type in NewFileTypes)
        {
            string? problem = null;
            if (string.IsNullOrWhiteSpace(type.Label))
                problem = "Every file type needs a name.";
            else if (!type.Extension.StartsWith('.') || type.Extension.Length < 2)
                problem = $"'{type.Label}' needs an extension starting with a dot, like \".txt\".";
            else if (RenamePattern.Validate("x" + type.Extension.Trim()) is { } invalid)
                problem = $"'{type.Label}' has an extension that can't end a file name — {invalid}";

            if (problem is not null)
            {
                // The offending row may be on a page the user cannot see, so go there first —
                // otherwise the message names a field that is nowhere on screen.
                SelectedNewFileType = type;
                ShowCategory(SettingsCategory.NewItems);
                error = problem;
                return false;
            }
        }

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
        // Always a list, never null, once this dialog has been saved: from here on the user has
        // configured it, and an empty one means they emptied it on purpose.
        _settings.NewFileTypes = NewFileTypes.Select(t => t.ToTemplate()).ToList();
        _settings.ShowHiddenItems = ShowHiddenItems;
        _settings.ScrollSpeedMultiplier = ScrollSpeed;
        _settings.TileAspectRatio = TileAspect.ToString();
        _settings.Save();
        error = null;
        return true;
    }
}
