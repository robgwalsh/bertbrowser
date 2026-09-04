using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Media;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Changes;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.ShellIntegration;

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
    Preview,
    History,
    NewItems,
    Columns,
    Commands,
}

/// <summary>One choice on the History page's "keep for" list.</summary>
public sealed record RetentionOption(int Hours, string Label)
{
    public static string LabelFor(int hours) => hours switch
    {
        1 => "1 hour",
        < 24 => $"{hours} hours",
        24 => "24 hours",
        _ => $"{hours / 24} days",
    };
}

/// <summary>An editable row of the default column list.</summary>
/// <remarks>
/// Carries the spec as well as the setting, so the list can show a person "Date taken" while what is
/// stored and compared is <c>System.Photo.DateTaken</c> — the canonical-name rule the whole feature
/// rests on.
/// </remarks>
public sealed partial class ColumnItemViewModel : ObservableObject
{
    public ColumnItemViewModel(ColumnSetting setting)
    {
        Id = setting.Id;
        Header = ColumnCatalog.TryGet(setting.Id)?.Header ?? setting.Id;
        _width = setting.Width;
    }

    public string Id { get; }

    public string Header { get; }

    /// <summary>Shown under the header so two similarly-named properties can be told apart, and
    /// because this is the string that ends up in settings.json.</summary>
    public string Detail =>
        ColumnCatalog.TryGet(Id) is { Kind: ColumnKind.ShellProperty } ? Id : "";

    /// <summary>Name is the one row that cannot be removed — it carries the icon and identifies the
    /// row. The Remove button asks this rather than the list finding out afterwards.</summary>
    public bool Removable => !string.Equals(Id, ColumnCatalog.Name, StringComparison.OrdinalIgnoreCase);

    [ObservableProperty]
    private double _width;

    public ColumnSetting ToSetting() => new(Id, Width);
}

/// <summary>One row of the navigation list.</summary>
public sealed class SettingsCategoryViewModel
{
    public SettingsCategoryViewModel(SettingsCategory id, string name, string iconKey)
    {
        Id = id;
        Name = name;
        IconKey = iconKey;
    }

    public SettingsCategory Id { get; }

    public string Name { get; }

    /// <summary>A key from Resources/Icons.xaml, named in tools/icon/icons.txt.</summary>
    public string IconKey { get; }

    /// <summary>
    /// The outline to draw, or null outside a running app — the settings list is the one place a
    /// view model picks an icon, because the category list is built here rather than in XAML.
    /// </summary>
    public Geometry? Icon => Application.Current?.TryFindResource(IconKey) as Geometry;
}

public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly AppSettings _settings;
    private readonly IShellNewCatalog? _shellNew;
    private readonly IFolderHandlerService? _folderHandler;

    /// <summary>Guards the live-apply handler against the revert it performs on failure, which
    /// would otherwise come straight back round as another change.</summary>
    private bool _applyingFolderHandler;

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

    /// <summary>Whether double-clicking an archive walks into it. See <c>AppSettings</c>.</summary>
    [ObservableProperty]
    private bool _enterArchivesOnDoubleClick;

    /// <summary>Whether picking a drive/device in the sidebar's Cards view opens a new panel
    /// instead of a new tab. See <c>AppSettings.DrivesOpenTarget</c>.</summary>
    [ObservableProperty]
    private bool _openDrivesInNewPanel;

    /// <summary>Whether startup restores the last arrangement, vs. always opening
    /// <see cref="StartupDefaultPath"/>. See <c>AppSettings.RestoreLastSession</c>.</summary>
    [ObservableProperty]
    private bool _restoreLastSession;

    /// <summary>Where startup opens when <see cref="RestoreLastSession"/> is off. See
    /// <c>AppSettings.StartupDefaultPath</c>.</summary>
    [ObservableProperty]
    private string _startupDefaultPath = "";

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

    // --- Preview ---

    /// <summary>Whether a newly opened tab starts with its preview showing. Visibility itself is
    /// per tab, so this is a default rather than a switch — which is why the page says so.</summary>
    [ObservableProperty]
    private bool _showPreviewPane;

    /// <summary>How much of a text file the preview reads, in kilobytes. Kept in KB here because
    /// that is the unit the number is worth typing in; the setting itself is bytes.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(PreviewTextLimitText))]
    private double _previewTextLimitKb;

    public string PreviewTextLimitText => $"{PreviewTextLimitKb:0} KB";

    /// <summary>How much of each file a <c>content:</c> search reads, in kilobytes.</summary>
    /// <remarks>Beside the preview budget because it is the same judgement about the same files.
    /// The other content-search ceilings stay constants — see <c>AppSettings</c> for why.</remarks>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ContentSearchLimitText))]
    private double _contentSearchLimitKb;

    public string ContentSearchLimitText => $"{ContentSearchLimitKb:0} KB";

    // --- History (the change timeline) ---

    private readonly ChangeLogRepository? _changeLog;

    /// <summary>Whether the index helper records file changes at all. Off by default; see
    /// <c>AppSettings.RecordFileChanges</c> for why, and why this has a page of its own.</summary>
    [ObservableProperty]
    private bool _recordFileChanges;

    /// <summary>How long recorded changes are kept — one of <see cref="RetentionOptions"/>.</summary>
    [ObservableProperty]
    private int _fileChangeRetentionHours;

    public IReadOnlyList<RetentionOption> RetentionOptions { get; } =
        ChangeLogPolicy.RetentionOptions.Select(h => new RetentionOption(h, RetentionOption.LabelFor(h))).ToList();

    /// <summary>"12,408 changes recorded" — what the switch is a switch over. Blank without a
    /// repository, which is only a construction site that did not pass one.</summary>
    [ObservableProperty]
    private string _recordedCountText = "";

    public bool CanClearHistory => _changeLog is not null;

    /// <summary>
    /// Deletes every recorded change, immediately.
    /// </summary>
    /// <remarks>
    /// Outside the Save/Cancel contract, like the theme picker and for a related reason: this is
    /// an action, not a preference, and a "clear" that Cancel could undo would not be one. The
    /// page says so beside the button.
    /// </remarks>
    [RelayCommand]
    private async Task ClearHistory()
    {
        if (_changeLog is null) return;
        await Task.Run(_changeLog.Clear);
        await RefreshRecordedCountAsync();
    }

    private async Task RefreshRecordedCountAsync()
    {
        if (_changeLog is null) return;
        try
        {
            var count = await Task.Run(_changeLog.Count);
            RecordedCountText = count == 1 ? "1 change recorded" : $"{count:N0} changes recorded";
        }
        catch (Microsoft.Data.Sqlite.SqliteException)
        {
            RecordedCountText = "";
        }
    }

    /// <summary>
    /// Theme selection and editing. Unlike everything else here it applies live rather than on
    /// Save — see the note in the dialog.
    /// </summary>
    public AppearanceViewModel Appearance { get; }

    // --- Opening folders (the Windows shell's Directory and Drive verbs) ---

    /// <summary>
    /// Whether Windows opens folders and drives in BertBrowser.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>This one applies immediately, like <see cref="Appearance"/> and unlike everything else on
    /// this page.</b> It is machine state rather than a stored preference — the registry is the
    /// single source of truth, and there is deliberately no mirrored flag in
    /// <c>AppSettings</c> for it to drift from. Cancel cannot un-write a registry key, so holding
    /// the change until Save would be a promise the dialog cannot keep.
    /// </para>
    /// </remarks>
    [ObservableProperty]
    private bool _openFoldersHere;

    /// <summary>False when there is no folder-handler service — a construction site that did not
    /// pass one, the way <see cref="NewFileTypes"/> import is hidden without a catalog.</summary>
    public bool CanChooseFolderHandler => _folderHandler is not null;

    /// <summary>Set when the registry refused a write, or when another program holds the verb.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasFolderHandlerWarning))]
    private string _folderHandlerWarning = "";

    public bool HasFolderHandlerWarning => FolderHandlerWarning.Length > 0;

    public SettingsViewModel(
        AppSettings settings,
        IThemeService theme,
        IShellNewCatalog? shellNew = null,
        IFolderHandlerService? folderHandler = null,
        ChangeLogRepository? changeLog = null)
    {
        Categories = new[]
        {
            new SettingsCategoryViewModel(SettingsCategory.General, "General", "Icon.Settings"),
            new SettingsCategoryViewModel(SettingsCategory.Appearance, "Appearance", "Icon.Appearance"),
            new SettingsCategoryViewModel(SettingsCategory.Preview, "Preview", "Icon.PreviewPane"),
            new SettingsCategoryViewModel(SettingsCategory.History, "History", "Icon.Changes"),
            new SettingsCategoryViewModel(SettingsCategory.NewItems, "New items", "Icon.Add"),
            new SettingsCategoryViewModel(SettingsCategory.Columns, "Columns", "Icon.Columns"),
            new SettingsCategoryViewModel(SettingsCategory.Commands, "Commands", "Icon.CustomCommand"),
        };
        _selectedCategory = Categories[0];

        Appearance = new AppearanceViewModel(theme);
        _shellNew = shellNew;
        _folderHandler = folderHandler;
        _settings = settings;

        // Read rather than restored: the registry is the state, so a change made outside this app
        // is simply what the box shows next time it opens.
        ReadFolderHandlerState();

        // Null means never configured, which is what ships the defaults; an empty list means the
        // user removed them all and must stay empty.
        NewFileTypes = new ObservableCollection<NewFileTypeItemViewModel>(
            (settings.NewFileTypes ?? NewFileTemplate.Defaults())
                .Select(t => new NewFileTypeItemViewModel(t)));
        SelectedNewFileType = NewFileTypes.FirstOrDefault();
        Columns = [];
        Rebuild(settings.ResolvedFileListColumns, ColumnCatalog.Name);
        ShowHiddenItems = settings.ShowHiddenItems;
        EnterArchivesOnDoubleClick = settings.EnterArchivesOnDoubleClick;
        OpenDrivesInNewPanel = settings.DrivesOpenTarget == DrivesOpenTarget.NewPanel;
        RestoreLastSession = settings.RestoreLastSession;
        StartupDefaultPath = settings.StartupDefaultPath ?? "";
        ScrollSpeed = settings.ScrollSpeedMultiplier;
        ShowPreviewPane = settings.ShowPreviewPane;
        PreviewTextLimitKb = Math.Round(settings.PreviewTextMaxBytes / 1024.0);
        ContentSearchLimitKb = Math.Round(settings.SearchContentMaxBytes / 1024.0);
        _changeLog = changeLog;
        RecordFileChanges = settings.RecordFileChanges;
        // A stored value off the menu would leave the box showing nothing; the policy the app runs
        // falls back the same way (AppSettings.ChangeLogPolicy).
        FileChangeRetentionHours = ChangeLogPolicy.IsAcceptableHours(settings.FileChangeRetentionHours) && settings.FileChangeRetentionHours > 0
            ? settings.FileChangeRetentionHours
            : ChangeLogPolicy.DefaultRetentionHours;
        _ = RefreshRecordedCountAsync();

        TileAspect = AspectRatio.Parse(settings.TileAspectRatio);
        TileAspectOptions = AspectRatio.Presets.Contains(TileAspect)
            ? AspectRatio.Presets
            : AspectRatio.Presets.Append(TileAspect).ToList();

        Commands = new ObservableCollection<CustomCommandItemViewModel>(
            settings.CustomCommands.Select(d => new CustomCommandItemViewModel(d)));
        SelectedCommand = Commands.FirstOrDefault();
    }

    // --- Opening folders ---

    /// <summary>
    /// Applies the change straight away, and says so if the registry refused rather than leaving a
    /// ticked box that means nothing.
    /// </summary>
    partial void OnOpenFoldersHereChanged(bool value)
    {
        if (_folderHandler is null || _applyingFolderHandler) return;

        if (_folderHandler.TrySet(value))
        {
            ReadFolderHandlerState();
            return;
        }

        _applyingFolderHandler = true;
        try
        {
            OpenFoldersHere = !value;
            FolderHandlerWarning = value
                ? "Windows would not let BertBrowser register as the folder handler."
                : "Windows would not let BertBrowser hand folders back to File Explorer.";
        }
        finally
        {
            _applyingFolderHandler = false;
        }
    }

    /// <summary>
    /// Seeds the box from what the registry actually says. The assignment is fenced by
    /// <see cref="_applyingFolderHandler"/> so reading the state never writes it back.
    /// </summary>
    private void ReadFolderHandlerState()
    {
        if (_folderHandler is null) return;

        var state = _folderHandler.State();

        _applyingFolderHandler = true;
        try
        {
            OpenFoldersHere = state is FolderHandlerState.RegisteredToThisApp
                or FolderHandlerState.RegisteredToThisAppStale;
        }
        finally
        {
            _applyingFolderHandler = false;
        }

        // Naming the other program rather than quietly taking the verb over. Ticking the box is
        // still allowed — that is the user asking — but they should know what it displaces.
        FolderHandlerWarning = state == FolderHandlerState.RegisteredToAnotherApp
            ? $"{Path.GetFileNameWithoutExtension(_folderHandler.OtherProgram()) ?? "Another program"} " +
              "currently opens folders. Turning this on will replace it."
            : "";
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

    // --- Columns ---
    //
    // The default set a new tab starts from. Every edit here goes through ColumnLayoutRules, the
    // same functions the header menu and a header drag use, so the three cannot disagree about what
    // adding or moving a column means — Name stays first whichever way it is asked for.

    /// <summary>The default columns, in the order they appear in a list.</summary>
    public ObservableCollection<ColumnItemViewModel> Columns { get; }

    [ObservableProperty]
    private ColumnItemViewModel? _selectedColumn;

    /// <summary>The layout as it stands, for the add popup to open onto and to build edits from.</summary>
    public IReadOnlyList<ColumnSetting> CurrentColumns() =>
        Columns.Select(c => c.ToSetting()).ToList();

    /// <summary>Takes one column from the add popup. Ids only: the popup offers properties this
    /// build has never heard of, which is the whole reason a column id is a string.</summary>
    public void AddColumn(string id) =>
        Rebuild(ColumnLayoutRules.Toggle(CurrentColumns(), id, on: true), id);

    /// <summary>
    /// Moves the row at <paramref name="from"/> to <paramref name="to"/> — what a drop reports.
    /// </summary>
    /// <remarks>
    /// The indexes go through <see cref="ColumnLayoutRules.Move"/> rather than being applied to the
    /// collection, so a row dropped above Name lands second and Name stays first. The list is
    /// rebuilt from that answer, which is what visually snaps such a drop back.
    /// </remarks>
    public void MoveColumn(int from, int to)
    {
        if (from < 0 || from >= Columns.Count) return;
        var moved = Columns[from];
        Rebuild(ColumnLayoutRules.Move(CurrentColumns(), moved.Id, to), moved.Id);
    }

    /// <summary>Moves the selected row, for Alt+Up and Alt+Down. The keyboard's half of the drag.</summary>
    public void NudgeSelectedColumn(int delta)
    {
        if (SelectedColumn is not { } selected) return;
        MoveColumn(Columns.IndexOf(selected), Columns.IndexOf(selected) + delta);
    }

    /// <summary>Name has no × on its row and is refused here too — the row's button is not the only
    /// way in, since Delete on the list comes through as well.</summary>
    [RelayCommand]
    private void RemoveColumn(ColumnItemViewModel? column)
    {
        if (column is not { Removable: true }) return;

        // Select what fell into the removed row's place, so pressing Delete twice removes two
        // columns rather than removing one and then nothing.
        var next = Math.Min(Columns.IndexOf(column), Columns.Count - 2);
        Rebuild(ColumnLayoutRules.Toggle(CurrentColumns(), column.Id, on: false), null);
        if (next >= 0 && next < Columns.Count) SelectedColumn = Columns[next];
    }

    [RelayCommand]
    private void ResetColumns() => Rebuild(ColumnCatalog.Defaults(), ColumnCatalog.Name);

    private void Rebuild(IReadOnlyList<ColumnSetting> settings, string? select)
    {
        Columns.Clear();
        foreach (var setting in settings)
            Columns.Add(new ColumnItemViewModel(setting));

        SelectedColumn = select is null
            ? Columns.FirstOrDefault()
            : Columns.FirstOrDefault(c => c.Id.Equals(select, StringComparison.OrdinalIgnoreCase));
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
        _settings.EnterArchivesOnDoubleClick = EnterArchivesOnDoubleClick;
        _settings.DrivesOpenTarget = OpenDrivesInNewPanel ? DrivesOpenTarget.NewPanel : DrivesOpenTarget.NewTab;
        _settings.RestoreLastSession = RestoreLastSession;
        _settings.StartupDefaultPath = StartupDefaultPath.Trim() is { Length: > 0 } path ? path : null;
        _settings.ScrollSpeedMultiplier = ScrollSpeed;
        _settings.ShowPreviewPane = ShowPreviewPane;
        _settings.PreviewTextMaxBytes = (int)Math.Clamp(PreviewTextLimitKb * 1024, 4096, 64 * 1024 * 1024);
        _settings.SearchContentMaxBytes = (int)Math.Clamp(ContentSearchLimitKb * 1024, 4096, 64 * 1024 * 1024);
        _settings.RecordFileChanges = RecordFileChanges;
        _settings.FileChangeRetentionHours = FileChangeRetentionHours;
        _settings.TileAspectRatio = TileAspect.ToString();
        // Always a list once this dialog has been saved, never null: from here on the user has
        // configured their columns, and the "never configured" state has nothing left to say.
        _settings.FileListColumns = ColumnLayoutRules.Normalize(CurrentColumns())
            .Select(c => c.Copy()).ToList();
        _settings.Save();
        error = null;
        return true;
    }
}
