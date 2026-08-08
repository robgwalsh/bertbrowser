using System.Collections.Specialized;
using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// One pane: a tab strip over several open directories, of which exactly one is visible. Any number
/// of these can be alive at once, arranged by <see cref="PaneLayoutHost"/>.
/// </summary>
public partial class FilePaneView : UserControl
{
    private readonly ShellViewModel _shell;
    private readonly AppSettings _settings;
    private readonly Dictionary<DirectoryTabViewModel, DirectoryTabView> _views = new();

    public PaneViewModel Pane { get; }

    public FilePaneView(ShellViewModel shell, AppSettings settings, PaneViewModel pane)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        Pane = pane;
        DataContext = pane;

        // Both are needed: keyboard-only traversal (F6) never raises a mouse event, and clicking
        // the tab strip's non-focusable chrome never raises a focus one.
        AddHandler(PreviewGotKeyboardFocusEvent,
            new KeyboardFocusChangedEventHandler(OnPreviewGotKeyboardFocus), handledEventsToo: true);
        AddHandler(PreviewMouseDownEvent,
            new MouseButtonEventHandler(OnPreviewMouseDown), handledEventsToo: true);

        Pane.Tabs.CollectionChanged += OnTabsChanged;
        Pane.PropertyChanged += OnPanePropertyChanged;

        foreach (var tab in Pane.Tabs)
            AddTabView(tab);
        UpdateVisibleTab();
        ClosePaneButton.IsEnabled = Pane.CanClosePane;
    }

    /// <summary>Gives back every subscription this pane and its tab views hold. Called when the
    /// pane is closed, or when the layout host drops it.</summary>
    public void Detach()
    {
        Pane.Tabs.CollectionChanged -= OnTabsChanged;
        Pane.PropertyChanged -= OnPanePropertyChanged;
        foreach (var view in _views.Values)
            view.Detach();
        _views.Clear();
        TabHost.Children.Clear();
    }

    public DirectoryTabView? ActiveTabView =>
        Pane.ActiveTab is { } tab && _views.TryGetValue(tab, out var view) ? view : null;

    public void FocusActiveTabList() => ActiveTabView?.FocusList();

    public void FocusSearchBox() => ActiveTabView?.FocusSearchBox();

    /// <summary>Refreshes the close button's availability; the last pane can't be closed, and which
    /// pane is last changes as others open and close.</summary>
    public void UpdateClosePaneAvailability() => ClosePaneButton.IsEnabled = Pane.CanClosePane;

    // --- Tab hosting ---

    private void OnTabsChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        foreach (var tab in e.NewItems?.OfType<DirectoryTabViewModel>() ?? [])
            AddTabView(tab);

        foreach (var tab in e.OldItems?.OfType<DirectoryTabViewModel>() ?? [])
        {
            if (!_views.Remove(tab, out var view)) continue;
            view.Detach();
            TabHost.Children.Remove(view);
        }

        UpdateVisibleTab();
    }

    private void AddTabView(DirectoryTabViewModel tab)
    {
        if (_views.ContainsKey(tab)) return;
        var view = new DirectoryTabView(_shell, _settings, tab) { Visibility = Visibility.Collapsed };
        _views[tab] = view;
        TabHost.Children.Add(view);
    }

    private void OnPanePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(PaneViewModel.ActiveTab))
            UpdateVisibleTab();
    }

    /// <summary>Shows the active tab and hides the rest. A collapsed view costs no measure, arrange
    /// or render pass, but keeps its list's scroll offset and selection intact.</summary>
    private void UpdateVisibleTab()
    {
        foreach (var (tab, view) in _views)
        {
            view.Visibility = ReferenceEquals(tab, Pane.ActiveTab)
                ? Visibility.Visible
                : Visibility.Collapsed;
        }
    }

    // --- Activation ---

    private void OnPreviewGotKeyboardFocus(object sender, KeyboardFocusChangedEventArgs e) =>
        _shell.ActivatePane(Pane);

    private void OnPreviewMouseDown(object sender, MouseButtonEventArgs e) =>
        _shell.ActivatePane(Pane);

    // --- Tab strip ---

    private void TabHeader_PreviewMouseDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is not FrameworkElement { DataContext: DirectoryTabViewModel tab }) return;

        if (e.ChangedButton == MouseButton.Middle)
        {
            Pane.CloseTabCommand.Execute(tab);
            e.Handled = true;
            return;
        }

        if (e.ChangedButton == MouseButton.Left || e.ChangedButton == MouseButton.Right)
            Pane.ActiveTab = tab;
    }

    /// <summary>The strip is a single row, so a wheel notch should walk it sideways rather than do
    /// nothing.</summary>
    private void TabStrip_PreviewMouseWheel(object sender, MouseWheelEventArgs e)
    {
        if (sender is not ScrollViewer scroller || e.Delta == 0) return;
        scroller.ScrollToHorizontalOffset(scroller.HorizontalOffset - e.Delta);
        e.Handled = true;
    }
}
