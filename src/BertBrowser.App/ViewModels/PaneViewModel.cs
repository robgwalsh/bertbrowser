using System.Collections.ObjectModel;
using System.Collections.Specialized;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Layout;

namespace BertBrowser.App.ViewModels;

/// <summary>What a pane needs from whoever owns the layout. Implemented by
/// <see cref="ShellViewModel"/>, so splitting and closing — the operations that reshape the tree —
/// happen in exactly one place instead of being spread across the panes themselves.</summary>
public interface IPaneHost
{
    void SplitPane(PaneViewModel pane, SplitOrientation orientation, string? path);
    void ClosePane(PaneViewModel pane);
    void ActivatePane(PaneViewModel pane);

    /// <summary>False when this is the last pane: the window always shows one.</summary>
    bool CanClosePane { get; }

    /// <summary>Told when a pane's visible directory changes, so the shell can drive the folder
    /// tree from the active pane only.</summary>
    void NotifyLocation(PaneViewModel pane, DirectoryTabViewModel tab);
}

/// <summary>
/// One pane: a tab strip over several open directories, of which exactly one is visible. Panes are
/// interchangeable — the window may hold any number of them in any arrangement — so a pane knows
/// only its own tabs and asks the host for anything involving its neighbours.
/// </summary>
public sealed partial class PaneViewModel : ObservableObject
{
    private readonly PaneFactory _factory;
    private readonly IPaneHost _host;

    public ObservableCollection<DirectoryTabViewModel> Tabs { get; } = new();

    [ObservableProperty]
    private DirectoryTabViewModel? _activeTab;

    /// <summary>True for the pane the window chrome follows; drives the highlight that tells the
    /// user which of several open directories their next keystroke lands in.</summary>
    [ObservableProperty]
    private bool _isActivePane;

    public bool HasMultipleTabs => Tabs.Count > 1;

    public PaneViewModel(PaneFactory factory, IPaneHost host)
    {
        _factory = factory;
        _host = host;
        Tabs.CollectionChanged += OnTabsChanged;
    }

    private void OnTabsChanged(object? sender, NotifyCollectionChangedEventArgs e) =>
        OnPropertyChanged(nameof(HasMultipleTabs));

    partial void OnActiveTabChanged(DirectoryTabViewModel? oldValue, DirectoryTabViewModel? newValue)
    {
        if (oldValue is not null) oldValue.IsActive = false;
        if (newValue is null) return;

        newValue.IsActive = true;
        newValue.OnActivated();
        _host.NotifyLocation(this, newValue);
    }

    /// <summary>Opens <paramref name="path"/> in a new tab of this pane. A background tab still
    /// loads — that is the point of opening one — it just doesn't take the foreground.</summary>
    public DirectoryTabViewModel AddTab(string path, bool activate = true)
    {
        var tab = ActiveTab is { } template ? _factory.CloneTab(template) : _factory.CreateTab();
        tab.LocationChanged += OnTabLocationChanged;
        Tabs.Add(tab);
        if (activate) ActiveTab = tab;
        if (path.Length > 0) _ = tab.NavigateToAsync(path);
        return tab;
    }

    private void OnTabLocationChanged(DirectoryTabViewModel tab)
    {
        if (ReferenceEquals(tab, ActiveTab))
            _host.NotifyLocation(this, tab);
    }

    [RelayCommand]
    private void NewTab(string? path) =>
        AddTab(path ?? ActiveTab?.CurrentPath ?? "", activate: true);

    [RelayCommand]
    private void DuplicateTab()
    {
        if (ActiveTab is { } tab) AddTab(tab.CurrentPath);
    }

    /// <summary>Puts the tab at <paramref name="from"/> into slot <paramref name="to"/>, as a drag
    /// along the strip does. The active tab stays active wherever it lands. An index outside the
    /// strip is ignored rather than thrown, because it only ever comes from a drop.</summary>
    public void MoveTab(int from, int to)
    {
        if (from == to) return;
        if (from < 0 || from >= Tabs.Count || to < 0 || to >= Tabs.Count) return;
        Tabs.Move(from, to);
    }

    /// <summary>Closes a tab, or — when it was the only one — the whole pane. Passing null closes
    /// the visible tab, which is what Ctrl+W and the strip's close glyph both mean.</summary>
    [RelayCommand]
    public void CloseTab(DirectoryTabViewModel? tab)
    {
        tab ??= ActiveTab;
        if (tab is null || !Tabs.Contains(tab)) return;

        if (Tabs.Count == 1)
        {
            _host.ClosePane(this);
            return;
        }

        var index = Tabs.IndexOf(tab);

        // Remembered before the tab is torn down, so Ctrl+Shift+T can put it back. Only the folder
        // is kept, not the tab: history and in-flight work belong to the object being disposed.
        if (tab.CurrentPath is { Length: > 0 } closedPath)
            RememberClosed(closedPath);

        Tabs.Remove(tab);
        tab.LocationChanged -= OnTabLocationChanged;
        tab.Dispose();

        if (ReferenceEquals(ActiveTab, tab) || ActiveTab is null)
            ActiveTab = Tabs[Math.Min(index, Tabs.Count - 1)];
    }

    /// <summary>Folders of recently closed tabs, most recent last.</summary>
    /// <remarks>
    /// Bounded because it is a convenience, not a history: an unbounded list would hold on to paths
    /// for the life of the session for no benefit anyone would notice.
    /// </remarks>
    private readonly List<string> _closedPaths = [];

    private const int MaxClosedPaths = 16;

    private void RememberClosed(string path)
    {
        _closedPaths.Add(path);
        if (_closedPaths.Count > MaxClosedPaths)
            _closedPaths.RemoveAt(0);
    }

    public bool CanReopenClosedTab => _closedPaths.Count > 0;

    /// <summary>Ctrl+Shift+T: reopens the most recently closed tab in this pane.</summary>
    /// <remarks>
    /// A no-op rather than an error when there is nothing to reopen — the same reading Ctrl+Z has
    /// with an empty undo slot. Closing the last tab closes the pane, so a pane that has gone takes
    /// its list with it; that is the one case this cannot reach back into.
    /// </remarks>
    [RelayCommand]
    public void ReopenClosedTab()
    {
        if (_closedPaths.Count == 0) return;

        var path = _closedPaths[^1];
        _closedPaths.RemoveAt(_closedPaths.Count - 1);
        AddTab(path);
    }

    [RelayCommand]
    private void CloseOtherTabs()
    {
        if (ActiveTab is not { } keep) return;
        foreach (var tab in Tabs.Where(t => !ReferenceEquals(t, keep)).ToList())
            CloseTab(tab);
    }

    [RelayCommand]
    private void NextTab() => StepTab(1);

    [RelayCommand]
    private void PreviousTab() => StepTab(-1);

    private void StepTab(int step)
    {
        if (Tabs.Count < 2 || ActiveTab is null) return;
        var index = Tabs.IndexOf(ActiveTab);
        ActiveTab = Tabs[((index + step) % Tabs.Count + Tabs.Count) % Tabs.Count];
    }

    /// <summary>Ctrl+1..9. The parameter is a string because that is all a XAML
    /// <c>KeyBinding.CommandParameter</c> supplies — an int overload would silently never run.
    /// By browser convention the last slot means "the last tab", however many there are.</summary>
    [RelayCommand]
    private void ActivateTabAt(string? index)
    {
        if (!int.TryParse(index, out var slot) || Tabs.Count == 0) return;
        ActiveTab = slot >= 8 || slot >= Tabs.Count ? Tabs[^1] : Tabs[Math.Max(0, slot)];
    }

    [RelayCommand]
    private void SplitVertical() => _host.SplitPane(this, SplitOrientation.Vertical, null);

    [RelayCommand]
    private void SplitHorizontal() => _host.SplitPane(this, SplitOrientation.Horizontal, null);

    public void SplitWith(string path, SplitOrientation orientation) =>
        _host.SplitPane(this, orientation, path);

    [RelayCommand]
    private void ClosePane() => _host.ClosePane(this);

    public bool CanClosePane => _host.CanClosePane;

    /// <summary>Releases every tab in the pane. Called when the pane itself is closed.</summary>
    public void Dispose()
    {
        foreach (var tab in Tabs)
        {
            tab.LocationChanged -= OnTabLocationChanged;
            tab.Dispose();
        }
        Tabs.Clear();
    }
}
