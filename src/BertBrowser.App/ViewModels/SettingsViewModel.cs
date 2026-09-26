using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Windows;
using System.Windows.Media;
using System.Windows.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Indexing;
using BertBrowser.App.Theming;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Changes;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.ShellIntegration;
using BertBrowser.Core.Services.ShellMenu;

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

/// <summary>A page of Settings, i.e. one entry in its left-hand navigation list.</summary>
public enum SettingsCategory
{
    General,
    Appearance,
    Preview,
    SearchIndex,
    History,
    NewItems,
    Columns,
    SavedSearches,
    Workspaces,
    ContextMenu,
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

/// <summary>One extension on the Context Menu page's checklist: 7-Zip, Git, TortoiseSVN…</summary>
public sealed partial class ShellExtensionItemViewModel : ObservableObject
{
    public ShellExtensionItemViewModel(ShellExtension extension, bool isShown)
    {
        Id = extension.Id;
        Name = extension.Name;
        KindText = extension.Kind == ShellExtensionKind.Handler ? "extension" : "command";
        _isShown = isShown;
    }

    /// <summary>The stable id the hidden list stores — see <see cref="ShellExtension.Id"/>.</summary>
    public string Id { get; }

    public string Name { get; }

    /// <summary>Shown beside the name so two entries called the same thing can be told apart: a
    /// program's COM handler ("extension") or a plain registered command.</summary>
    public string KindText { get; }

    [ObservableProperty]
    private bool _isShown;
}

/// <summary>One of the app's own entries on the Context Menu page's checklist: Open, Copy as
/// path, Analyse disk usage… Same box as an extension's, same meaning.</summary>
public sealed partial class BuiltInMenuItemViewModel : ObservableObject
{
    public BuiltInMenuItemViewModel(BuiltInMenuItem item, bool isShown)
    {
        Id = item.Id;
        Name = item.Name;
        PlacesText = BuiltInMenuItems.PlacesText(item.Places);
        _isShown = isShown;
    }

    /// <summary>The stable id the hidden list stores — see <see cref="BuiltInMenuItem.Id"/>.</summary>
    public string Id { get; }

    public string Name { get; }

    /// <summary>Where the entry appears, since not every verb is in both menus.</summary>
    public string PlacesText { get; }

    [ObservableProperty]
    private bool _isShown;
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

    /// <summary>Whether middle-clicking a drive/device in the sidebar opens a new panel instead
    /// of a new tab. See <c>AppSettings.DrivesOpenTarget</c>.</summary>
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
    /// typed in by hand would be silently replaced the first time settings are saved.</summary>
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
    /// An action rather than a preference, so it is not part of <see cref="Apply"/> and never waits
    /// on its debounce. The page says so beside the button.
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
    /// Theme selection and editing. Applied and persisted by the theme service itself rather than
    /// by <see cref="Apply"/>.
    /// </summary>
    public AppearanceViewModel Appearance { get; }

    // --- Opening folders (the Windows shell's Directory and Drive verbs) ---

    /// <summary>
    /// Whether Windows opens folders and drives in BertBrowser.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Written straight to the registry rather than by <see cref="Apply"/>.</b> It is machine
    /// state rather than a stored preference — the registry is the single source of truth, and
    /// there is deliberately no mirrored flag in <c>AppSettings</c> for it to drift from.
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

    // --- Search index ---

    private readonly IndexAutoStartService? _autoStart;
    private readonly IMftIndexService? _mftIndex;
    private bool _applyingAutoStart;

    /// <summary>Whether launching the app may raise the elevation prompt itself. Off by default.</summary>
    [ObservableProperty]
    private bool _startIndexerAtLaunch;

    /// <summary>
    /// Whether the helper is registered to start at sign-in. Applied immediately, like the folder
    /// handler — the scheduled task is the state, so there is nothing here for <c>Apply</c> to
    /// write.
    /// </summary>
    [ObservableProperty]
    private bool _startIndexerAtLogon;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasAutoStartWarning))]
    private string _autoStartWarning = "";

    public bool HasAutoStartWarning => AutoStartWarning.Length > 0;

    /// <summary>False when no service was passed — a construction site, or the UI harness.</summary>
    public bool CanChooseAutoStart => _autoStart is not null;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanStopIndexer))]
    [NotifyPropertyChangedFor(nameof(CanStartIndexerNow))]
    private string _indexerStatusText = "";

    /// <summary>
    /// Stop is only offered when there is a session to send Shutdown down.
    /// </summary>
    /// <remarks>
    /// A helper that is running but not answering cannot be stopped from here — nothing else can
    /// reach it — so the button is disabled and says so, rather than being pressed and doing
    /// nothing.
    /// </remarks>
    public bool CanStopIndexer => _mftIndex?.Presence == IndexerPresence.Running && !_indexerUnreachable;

    public bool CanStartIndexerNow => _mftIndex?.CanStart == true;

    private bool _indexerUnreachable;

    [RelayCommand]
    private void StartIndexerNow()
    {
        _mftIndex?.Start(IndexStartMode.AttachOrLaunch);
        ReadIndexerState();
    }

    [RelayCommand]
    private void StopIndexer()
    {
        _mftIndex?.Stop();
        ReadIndexerState();
    }

    /// <summary>What the "Background indexer" line says, read from the service each time.</summary>
    public void ReadIndexerState()
    {
        if (_mftIndex is null)
        {
            IndexerStatusText = "";
            return;
        }

        // A failure message means a helper we cannot talk to, whether or not one is running.
        _indexerUnreachable = _mftIndex.StatusText.Contains("not responding", StringComparison.OrdinalIgnoreCase);

        IndexerStatusText = _mftIndex.Presence switch
        {
            IndexerPresence.Running when _indexerUnreachable => "Running, but not responding.",
            IndexerPresence.Running => "Running.",
            IndexerPresence.NotRunning => "Not running.",
            _ => "",
        };

        OnPropertyChanged(nameof(CanStopIndexer));
        OnPropertyChanged(nameof(CanStartIndexerNow));
    }

    /// <summary>
    /// Applies the sign-in task straight away, and puts the box back if it did not happen — a
    /// declined prompt included. The fence stops the correction re-entering this handler.
    /// </summary>
    partial void OnStartIndexerAtLogonChanged(bool value)
    {
        if (_autoStart is null || _applyingAutoStart) return;

        if (_autoStart.TrySet(value))
        {
            ReadAutoStartState();
            return;
        }

        _applyingAutoStart = true;
        try
        {
            StartIndexerAtLogon = !value;
            AutoStartWarning = value
                ? "The sign-in task could not be created. Administrator rights are needed to add one."
                : "The sign-in task could not be removed.";
        }
        finally
        {
            _applyingAutoStart = false;
        }
    }

    /// <summary>
    /// Seeds the box from the scheduler.
    /// </summary>
    /// <remarks>
    /// <b>An unreadable task shows as off, never on.</b> A ticked box the app could not verify is a
    /// promise it has not checked it can keep — and the cost of being wrong that way is a user who
    /// believes the prompt has been abolished and is surprised at the next sign-in.
    /// </remarks>
    private void ReadAutoStartState()
    {
        if (_autoStart is null) return;

        var state = _autoStart.State();

        _applyingAutoStart = true;
        try
        {
            StartIndexerAtLogon = state == AutoStartState.On;
        }
        finally
        {
            _applyingAutoStart = false;
        }

        AutoStartWarning = state switch
        {
            AutoStartState.Stale =>
                "The sign-in task points at an older installation. Tick this again to repair it.",
            AutoStartState.Unknown =>
                "Whether a sign-in task exists could not be read.",
            _ => "",
        };
    }

    public SettingsViewModel(
        AppSettings settings,
        IThemeService theme,
        IShellNewCatalog? shellNew = null,
        IFolderHandlerService? folderHandler = null,
        ChangeLogRepository? changeLog = null,
        IndexAutoStartService? autoStart = null,
        IMftIndexService? mftIndex = null,
        IShellMenuSource? shellMenus = null,
        SavedWorkspacesViewModel? workspaces = null,
        SavedSearchesViewModel? savedSearches = null)
    {
        Categories = new[]
        {
            new SettingsCategoryViewModel(SettingsCategory.General, "General", "Icon.Settings"),
            new SettingsCategoryViewModel(SettingsCategory.Appearance, "Appearance", "Icon.Appearance"),
            new SettingsCategoryViewModel(SettingsCategory.Preview, "Preview", "Icon.PreviewPane"),
            new SettingsCategoryViewModel(SettingsCategory.SearchIndex, "Search index", "Icon.Indexing"),
            new SettingsCategoryViewModel(SettingsCategory.History, "History", "Icon.Changes"),
            new SettingsCategoryViewModel(SettingsCategory.NewItems, "New items", "Icon.Add"),
            new SettingsCategoryViewModel(SettingsCategory.Columns, "Columns", "Icon.Columns"),
            new SettingsCategoryViewModel(SettingsCategory.SavedSearches, "Saved searches", "Icon.Search"),
            new SettingsCategoryViewModel(SettingsCategory.Workspaces, "Workspaces", "Icon.SplitRight"),
            new SettingsCategoryViewModel(SettingsCategory.ContextMenu, "Context menu", "Icon.CustomCommand"),
        };
        _selectedCategory = Categories[0];

        Appearance = new AppearanceViewModel(theme);
        _shellNew = shellNew;
        _folderHandler = folderHandler;
        _settings = settings;

        // Read rather than restored: the registry is the state, so a change made outside this app
        // is simply what the box shows next time it opens.
        ReadFolderHandlerState();

        _autoStart = autoStart;
        _mftIndex = mftIndex;
        // Same discipline as the folder handler, for the same reason: the scheduled task is the
        // state, so this reads it rather than trusting anything stored beside it.
        ReadAutoStartState();
        StartIndexerAtLaunch = settings.StartIndexerAtLaunch;

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

        var hiddenBuiltIn = new HashSet<string>(settings.HiddenBuiltInMenuItems, StringComparer.OrdinalIgnoreCase);
        BuiltInItems = new ObservableCollection<BuiltInMenuItemViewModel>(
            BuiltInMenuItems.All.Select(i => new BuiltInMenuItemViewModel(i, isShown: !hiddenBuiltIn.Contains(i.Id))));

        _shellMenus = shellMenus;
        ShowShellExtensions = settings.ShowShellExtensions;
        WorkspacesPlacement = settings.WorkspacesPlacement;
        Workspaces = workspaces;
        SavedSearchesPlacement = settings.SavedSearchesPlacement;
        SavedSearches = savedSearches;
        _ = LoadShellExtensionsAsync();

        TrackChanges();
    }

    // --- Workspaces ---

    /// <summary>Where the switcher lives: sidebar, title bar, or nowhere. See
    /// <c>AppSettings.WorkspacesPlacement</c>.</summary>
    [ObservableProperty]
    private SectionPlacement _workspacesPlacement;

    /// <summary>
    /// The shell's own list, not a copy, so a rename here is already in the sidebar when the page
    /// closes. Renaming and deleting go through the shell too — they are actions rather than
    /// preferences, so they never wait on <see cref="Apply"/>'s debounce. Null only at a
    /// construction site that did not pass one.
    /// </summary>
    public SavedWorkspacesViewModel? Workspaces { get; }

    [ObservableProperty]
    private SavedWorkspaceItemViewModel? _selectedWorkspace;

    // --- Saved searches ---
    //
    // The same arrangement as the workspaces above: a placement that Apply writes, and the shell's
    // own list, edited and deleted through the shell.

    [ObservableProperty]
    private SectionPlacement _savedSearchesPlacement;

    public SavedSearchesViewModel? SavedSearches { get; }

    [ObservableProperty]
    private SavedSearchItemViewModel? _selectedSavedSearch;

    // --- The app's own context-menu entries ---

    /// <summary>Every entry the file list's and folder tree's menus have, each with whether it is
    /// shown. Known at compile time, unlike the extensions, so it needs no scan.</summary>
    public ObservableCollection<BuiltInMenuItemViewModel> BuiltInItems { get; }

    // --- Other programs' context-menu entries ---

    private readonly IShellMenuSource? _shellMenus;

    /// <summary>Every extension found on this machine, each with whether it is shown. Filled after
    /// construction: the scan walks all of HKEY_CLASSES_ROOT and runs off the UI thread.</summary>
    public ObservableCollection<ShellExtensionItemViewModel> ShellExtensions { get; } = [];

    /// <summary>The master switch; see <c>AppSettings.ShowShellExtensions</c>.</summary>
    [ObservableProperty]
    private bool _showShellExtensions;

    /// <summary>"Looking for extensions…" while the scan runs, a note when it finds none, else blank.</summary>
    [ObservableProperty]
    private string _shellExtensionsStatus = "";

    private async Task LoadShellExtensionsAsync()
    {
        if (_shellMenus is null) return;

        ShellExtensionsStatus = "Looking for extensions…";
        var catalog = await _shellMenus.CatalogAsync();

        var hidden = new HashSet<string>(_settings.HiddenShellExtensions, StringComparer.OrdinalIgnoreCase);
        foreach (var extension in catalog)
        {
            var item = new ShellExtensionItemViewModel(extension, isShown: !hidden.Contains(extension.Id));
            item.PropertyChanged += OnShownChanged;
            ShellExtensions.Add(item);
        }

        ShellExtensionsStatus = catalog.Count == 0
            ? "No program on this computer adds right-click entries."
            : "";
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
    /// page shows one category at a time.</summary>
    public void ShowCategory(SettingsCategory category)
    {
        SelectedCategory = Categories.First(c => c.Id == category);
    }

    // --- Applying ---
    //
    // There is no Save: the page writes settings.json as things change, on a short debounce because
    // the text boxes update per keystroke and every apply also re-lays out every tab's columns.
    // What triggers one is an allowlist, not "any property changed", so moving between pages or
    // selecting a row never writes anything.

    private static readonly HashSet<string> PersistedProperties =
    [
        nameof(ShowHiddenItems), nameof(EnterArchivesOnDoubleClick), nameof(OpenDrivesInNewPanel),
        nameof(RestoreLastSession), nameof(StartupDefaultPath), nameof(ScrollSpeed), nameof(TileAspect),
        nameof(ShowPreviewPane), nameof(PreviewTextLimitKb), nameof(ContentSearchLimitKb),
        nameof(RecordFileChanges), nameof(FileChangeRetentionHours), nameof(StartIndexerAtLaunch),
        nameof(ShowShellExtensions), nameof(WorkspacesPlacement), nameof(SavedSearchesPlacement),
    ];

    private DispatcherTimer? _applyTimer;

    /// <summary>Raised after each write, for the host to push what changed through the shell.</summary>
    public event EventHandler? Applied;

    /// <summary>
    /// Why a list is not being written, e.g. a command with no program yet. Null when everything
    /// on the page has been saved.
    /// </summary>
    [ObservableProperty]
    private string? _pendingProblem;

    /// <summary>Called last in the constructor, so seeding the fields from settings is not itself
    /// a change.</summary>
    private void TrackChanges()
    {
        _applyTimer = new DispatcherTimer(DispatcherPriority.Background) { Interval = TimeSpan.FromMilliseconds(400) };
        _applyTimer.Tick += (_, _) => Apply();

        PropertyChanged += (_, e) =>
        {
            if (e.PropertyName is { } name && PersistedProperties.Contains(name)) ScheduleApply();
        };

        Track(Commands);
        Track(NewFileTypes);
        Track(Columns);

        // Only the tick counts on these two. The extension list fills in after construction, and
        // that scan finishing is not something the user changed.
        foreach (var item in BuiltInItems) item.PropertyChanged += OnShownChanged;
    }

    private void Track<T>(ObservableCollection<T> list) where T : INotifyPropertyChanged
    {
        foreach (var item in list) item.PropertyChanged += OnItemChanged;
        list.CollectionChanged += (_, e) =>
        {
            foreach (var item in e.NewItems?.OfType<T>() ?? []) item.PropertyChanged += OnItemChanged;
            foreach (var item in e.OldItems?.OfType<T>() ?? []) item.PropertyChanged -= OnItemChanged;
            ScheduleApply();
        };
    }

    private void OnItemChanged(object? sender, PropertyChangedEventArgs e) => ScheduleApply();

    private void OnShownChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(BuiltInMenuItemViewModel.IsShown)) ScheduleApply();
    }

    private void ScheduleApply()
    {
        if (_applyTimer is null) return;
        _applyTimer.Stop();
        _applyTimer.Start();
    }

    /// <summary>Writes anything still waiting on the debounce. For leaving the page and closing the
    /// window, where a change made in the last moment would otherwise be lost.</summary>
    public void Flush()
    {
        if (_applyTimer?.IsEnabled == true) Apply();
    }

    /// <summary>
    /// Flushes, then says whether anything on the page could not be saved.
    /// </summary>
    /// <remarks>
    /// Only here does an incomplete entry get brought on screen. While the page is open it would
    /// jump pages under someone halfway through typing it.
    /// </remarks>
    public bool TryLeave(out string? problem)
    {
        Flush();

        if (NewFileTypeProblem() is { } type)
        {
            SelectedNewFileType = type.Item;
            ShowCategory(SettingsCategory.NewItems);
            problem = type.Message;
            return false;
        }

        if (CommandProblem() is { } command)
        {
            SelectedCommand = command.Item;
            ShowCategory(SettingsCategory.ContextMenu);
            problem = command.Message;
            return false;
        }

        problem = null;
        return true;
    }

    private (NewFileTypeItemViewModel Item, string Message)? NewFileTypeProblem()
    {
        foreach (var type in NewFileTypes)
        {
            if (string.IsNullOrWhiteSpace(type.Label))
                return (type, "Every file type needs a name.");
            if (!type.Extension.StartsWith('.') || type.Extension.Length < 2)
                return (type, $"'{type.Label}' needs an extension starting with a dot, like \".txt\".");
            if (RenamePattern.Validate("x" + type.Extension.Trim()) is { } invalid)
                return (type, $"'{type.Label}' has an extension that can't end a file name — {invalid}");
        }
        return null;
    }

    private (CustomCommandItemViewModel Item, string Message)? CommandProblem()
    {
        foreach (var command in Commands)
        {
            if (string.IsNullOrWhiteSpace(command.Name) || string.IsNullOrWhiteSpace(command.Command))
                return (command, "Every command needs a name and a program.");
            if (!command.AppliesToFiles && !command.AppliesToDirectories)
                return (command, $"'{command.Name}' must apply to files, folders, or both.");
        }
        return null;
    }

    /// <summary>
    /// Writes the page to settings.json.
    /// </summary>
    /// <remarks>
    /// The new-file types and the commands are each written only when every entry in the list is
    /// complete. Until then the list already saved stays in force and <see cref="PendingProblem"/>
    /// says why. Everything else is always written.
    /// </remarks>
    public void Apply()
    {
        _applyTimer?.Stop();

        var typeProblem = NewFileTypeProblem();
        var commandProblem = CommandProblem();
        PendingProblem = (typeProblem?.Message ?? commandProblem?.Message) is { } message
            ? $"{message} That list is not saved until this is fixed."
            : null;

        if (commandProblem is null)
            _settings.CustomCommands = Commands.Select(c => c.ToDefinition()).ToList();
        _settings.ShowShellExtensions = ShowShellExtensions;
        // Ids hidden earlier but not found today stay hidden: the list may still be loading, or the
        // extension may be uninstalled for now. See ShellMenuRules.HiddenAfterSave.
        _settings.HiddenShellExtensions = ShellMenuRules.HiddenAfterSave(
            _settings.HiddenShellExtensions,
            ShellExtensions.Select(e => (e.Id, e.IsShown))).ToList();
        // Same rule for the app's own entries: an id another version of the app knows and this
        // one does not is kept, not dropped.
        _settings.HiddenBuiltInMenuItems = ShellMenuRules.HiddenAfterSave(
            _settings.HiddenBuiltInMenuItems,
            BuiltInItems.Select(e => (e.Id, e.IsShown))).ToList();
        // Always a list, never null, once settings have been saved: from here on the user has
        // configured it, and an empty one means they emptied it on purpose.
        if (typeProblem is null)
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
        // The only search-index value that lives in settings. Sign-in auto-start is the scheduled
        // task itself and was already applied when the box was ticked.
        _settings.StartIndexerAtLaunch = StartIndexerAtLaunch;
        _settings.WorkspacesPlacement = WorkspacesPlacement;
        _settings.SavedSearchesPlacement = SavedSearchesPlacement;
        _settings.TileAspectRatio = TileAspect.ToString();
        // Always a list once settings have been saved, never null: from here on the user has
        // configured their columns, and the "never configured" state has nothing left to say.
        _settings.FileListColumns = ColumnLayoutRules.Normalize(CurrentColumns())
            .Select(c => c.Copy()).ToList();
        _settings.Save();
        Applied?.Invoke(this, EventArgs.Empty);
    }
}
