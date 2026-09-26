using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Columns;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Win32;

namespace BertBrowser.App.Views;

/// <summary>
/// The settings page. It sits in the main window in place of the folders; <see cref="MainWindow"/>
/// shows it, and it goes back when this raises <see cref="BackRequested"/>.
/// </summary>
public partial class SettingsView : UserControl
{
    private readonly SettingsViewModel _vm;

    /// <summary>Back, or Esc. The host decides whether leaving is allowed (see
    /// <see cref="SettingsViewModel.TryLeave"/>).</summary>
    public event EventHandler? BackRequested;

    /// <summary>"Customise colours…". The editor is modeless so its changes can be judged against
    /// the file list, so the host has to put the folders back before opening it.</summary>
    public event EventHandler? CustomiseThemeRequested;

    public SettingsViewModel ViewModel => _vm;

    public SettingsView(SettingsViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;

        // The reorder that replaced the up and down buttons. The drop reports two indexes; what
        // they mean is ColumnLayoutRules' business, not this view's.
        ListReorderDrag.Attach(ColumnDefaultsList, Orientation.Vertical, _vm.MoveColumn);

        // While "Match Windows light/dark" is on, the theme can change with this page open —
        // Windows flipping at sunset — and the pickers have to follow it. Subscribed here rather
        // than in the view model because nothing disposes one of those and IThemeService is a
        // singleton, so a view-model subscription would leak a graph per Settings open. Loaded and
        // Unloaded rather than the constructor: the host drops this view when it goes back.
        Loaded += (_, _) => WatchThemeChanges(true);
        Unloaded += (_, _) => WatchThemeChanges(false);
    }

    /// <summary>Puts the keyboard on the category list, so arrows walk the pages straight away.</summary>
    public void FocusCategories()
    {
        if (!IsLoaded)
        {
            Loaded += FocusOnLoad;
            return;
        }

        CategoryList.UpdateLayout();
        var row = CategoryList.ItemContainerGenerator.ContainerFromItem(_vm.SelectedCategory) as ListBoxItem;
        if (row is not null) row.Focus();
        else CategoryList.Focus();
    }

    private void FocusOnLoad(object sender, RoutedEventArgs e)
    {
        Loaded -= FocusOnLoad;
        FocusCategories();
    }

    private Theming.IThemeService? _watchedTheme;

    private void WatchThemeChanges(bool on)
    {
        if (on && _watchedTheme is null && App.Services?.GetService<Theming.IThemeService>() is { } theme)
        {
            _watchedTheme = theme;
            theme.ThemeChanged += OnThemeChanged;
        }
        else if (!on && _watchedTheme is { } watched)
        {
            watched.ThemeChanged -= OnThemeChanged;
            _watchedTheme = null;
        }
    }

    private void OnThemeChanged(object? sender, EventArgs e) => _vm.Appearance.SyncAfterExternalChange();

    /// <summary>Keeps an <see cref="AppearanceViewModel"/> in step with theme changes it did not
    /// cause, for the life of a window. Used by <see cref="ThemeEditorWindow"/>.</summary>
    internal static void WatchThemeChanges(Window window, AppearanceViewModel appearance)
    {
        // Resolved rather than injected, the route ThemedWindow itself uses for this.
        if (App.Services?.GetService<Theming.IThemeService>() is not { } theme) return;

        void OnThemeChanged(object? sender, EventArgs e) => appearance.SyncAfterExternalChange();

        theme.ThemeChanged += OnThemeChanged;
        window.Closed += (_, _) => theme.ThemeChanged -= OnThemeChanged;
    }

    private void Back_Click(object sender, RoutedEventArgs e) => BackRequested?.Invoke(this, EventArgs.Empty);

    /// <summary>
    /// Esc goes back. Bubbling rather than Preview, so an open combo box or the column popup takes
    /// its own Esc first and only an unclaimed one leaves the page.
    /// </summary>
    private void View_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape || e.Handled || Keyboard.Modifiers != ModifierKeys.None) return;
        e.Handled = true;
        BackRequested?.Invoke(this, EventArgs.Empty);
    }

    private void CustomiseTheme_Click(object sender, RoutedEventArgs e) =>
        CustomiseThemeRequested?.Invoke(this, EventArgs.Empty);

    /// <summary>A workspace's Rename, by button or F2. The host asks for the name, since the dialog
    /// and the shell call are the same ones the sidebar uses.</summary>
    public event EventHandler<SavedWorkspaceItemViewModel>? WorkspaceRenameRequested;

    /// <summary>A workspace's Delete, by button or the Delete key.</summary>
    public event EventHandler<SavedWorkspaceItemViewModel>? WorkspaceDeleteRequested;

    private void WorkspaceRename_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is SavedWorkspaceItemViewModel item)
            WorkspaceRenameRequested?.Invoke(this, item);
    }

    private void WorkspaceDelete_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is SavedWorkspaceItemViewModel item)
            WorkspaceDeleteRequested?.Invoke(this, item);
    }

    private void WorkspaceList_KeyDown(object sender, KeyEventArgs e)
    {
        if (_vm.SelectedWorkspace is { } item && _vm.Workspaces is { } workspaces)
            ListKeyDown(e, WorkspaceList, workspaces.Items,
                () => WorkspaceRenameRequested?.Invoke(this, item),
                () => WorkspaceDeleteRequested?.Invoke(this, item));
    }

    /// <summary>A saved search's Edit, by button or F2 — the sidebar's Edit dialog, which renames
    /// as well as changing the query and scope.</summary>
    public event EventHandler<SavedSearchItemViewModel>? SavedSearchEditRequested;

    public event EventHandler<SavedSearchItemViewModel>? SavedSearchDeleteRequested;

    private void SavedSearchEdit_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is SavedSearchItemViewModel item)
            SavedSearchEditRequested?.Invoke(this, item);
    }

    private void SavedSearchDelete_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is SavedSearchItemViewModel item)
            SavedSearchDeleteRequested?.Invoke(this, item);
    }

    private void SavedSearchList_KeyDown(object sender, KeyEventArgs e)
    {
        if (_vm.SelectedSavedSearch is { } item && _vm.SavedSearches is { } searches)
            ListKeyDown(e, SavedSearchList, searches.Items,
                () => SavedSearchEditRequested?.Invoke(this, item),
                () => SavedSearchDeleteRequested?.Invoke(this, item));
    }

    /// <summary>F2 and Delete act on the selected row, as they do on a file. After a delete, focus
    /// goes to whichever row took the removed one's place, so pressing Delete twice deletes two.</summary>
    private void ListKeyDown<T>(KeyEventArgs e, ListBox list, ObservableCollection<T> items, Action edit, Action delete)
    {
        if (Keyboard.Modifiers != ModifierKeys.None) return;

        switch (e.Key)
        {
            case Key.F2:
                e.Handled = true;
                edit();
                break;
            case Key.Delete:
                e.Handled = true;
                ReselectAfterRemoval(list, items, list.SelectedIndex);
                delete();
                break;
        }
    }

    /// <summary>The delete lands after a database round trip, so the next row is chosen when the
    /// removed one actually leaves the list rather than on a guess at when that will be.</summary>
    private void ReselectAfterRemoval<T>(ListBox list, ObservableCollection<T> items, int index)
    {
        void OnChanged(object? s, NotifyCollectionChangedEventArgs e)
        {
            if (e.Action != NotifyCollectionChangedAction.Remove) return;
            items.CollectionChanged -= OnChanged;
            Dispatcher.BeginInvoke(() =>
            {
                if (items.Count == 0) return;
                var next = items[Math.Clamp(index, 0, items.Count - 1)];
                list.SelectedItem = next;
                list.UpdateLayout();
                (list.ItemContainerGenerator.ContainerFromItem(next) as ListBoxItem)?.Focus();
            }, System.Windows.Threading.DispatcherPriority.Background);
        }

        items.CollectionChanged += OnChanged;
    }


    /// <summary>
    /// The whole property system, for the columns the curated list does not name.
    /// </summary>
    /// <remarks>
    /// A popup hung off the button rather than a modal dialog, and the only way in: there is no
    /// second "More columns…" button because the popup's own search box is what reaches past the
    /// curated list. Each click writes straight into the pending layout, which is what lets three
    /// columns be added in three clicks with nothing to confirm.
    /// </remarks>
    private void AddColumn_Click(object sender, RoutedEventArgs e) =>
        ColumnAddPopup.Show(AddColumnButton, _vm.CurrentColumns, _vm.AddColumn);

    /// <summary>
    /// The keyboard's half of the drag, plus Delete.
    /// </summary>
    /// <remarks>
    /// Alt is what makes Up and Down mean "move this" rather than "select the next one" — the same
    /// modifier every list that can be reordered by hand uses, and the reason plain arrows still
    /// walk the list.
    /// </remarks>
    private void ColumnList_KeyDown(object sender, KeyEventArgs e)
    {
        if (Keyboard.Modifiers == ModifierKeys.Alt && e.Key is Key.Up or Key.Down or Key.System)
        {
            // Alt turns the key into Key.System and puts the real one on SystemKey.
            var key = e.Key == Key.System ? e.SystemKey : e.Key;
            if (key is not (Key.Up or Key.Down)) return;

            _vm.NudgeSelectedColumn(key == Key.Up ? -1 : 1);
            RefocusSelectedColumn();
            e.Handled = true;
        }
        else if (e.Key == Key.Delete && _vm.SelectedColumn is { Removable: true } selected)
        {
            _vm.RemoveColumnCommand.Execute(selected);
            RefocusSelectedColumn();
            e.Handled = true;
        }
    }

    /// <summary>
    /// Puts the keyboard back on the selected row.
    /// </summary>
    /// <remarks>
    /// Every edit rebuilds the list from what the layout rules answered, which throws away the
    /// containers — and with them the focus this handler is reached through. Without this, Alt+Up
    /// would move a column once and then go silent.
    /// </remarks>
    private void RefocusSelectedColumn()
    {
        ColumnDefaultsList.UpdateLayout();
        if (_vm.SelectedColumn is not { } selected) return;
        if (ColumnDefaultsList.ItemContainerGenerator.ContainerFromItem(selected) is ListBoxItem row)
            row.Focus();
    }

    /// <summary>
    /// The wheel over a width box changes the width instead of scrolling the list past it.
    /// </summary>
    /// <remarks>
    /// Handled unconditionally, including at the ends of the range: letting the scroll through once
    /// the width has hit its limit would make the list lurch under a pointer that had been sitting
    /// still, which reads as a fault rather than as a limit.
    /// </remarks>
    private void ColumnWidth_Wheel(object sender, MouseWheelEventArgs e)
    {
        e.Handled = true;
        if (sender is not FrameworkElement { DataContext: ColumnItemViewModel column }) return;

        var notches = e.Delta / Mouse.MouseWheelDeltaForOneLine;
        column.Width = ColumnLayoutRules.StepWidth(
            column.Width, notches, fine: Keyboard.Modifiers.HasFlag(ModifierKeys.Control));
    }

    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedCommand is not { } command) return;

        var dialog = new OpenFileDialog
        {
            Title = "Choose a program",
            Filter = "Programs (*.exe;*.bat;*.cmd)|*.exe;*.bat;*.cmd|All files (*.*)|*.*",
        };
        if (dialog.ShowDialog(Window.GetWindow(this)) == true)
            command.Command = dialog.FileName;
    }

    private void BrowseTemplate_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedNewFileType is not { } type) return;

        var dialog = new OpenFileDialog
        {
            Title = "Choose a template file",
            Filter = "All files (*.*)|*.*",
        };
        if (dialog.ShowDialog(Window.GetWindow(this)) == true)
            type.TemplatePath = dialog.FileName;
    }

    private void BrowseStartupPath_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFolderDialog
        {
            Title = "Choose a startup folder",
            InitialDirectory = _vm.StartupDefaultPath,
        };
        if (dialog.ShowDialog(Window.GetWindow(this)) == true)
            _vm.StartupDefaultPath = dialog.FolderName;
    }
}
