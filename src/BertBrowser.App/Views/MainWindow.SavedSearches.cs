using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>Saved searches wherever Settings puts them — the sidebar's section or the title bar's
/// dropdown — plus the header box's save button and the edit and delete the Settings page asks for.
/// Each handler is a line into the shell, which owns what running, saving and deleting one means.
/// The dialog is shown here, as every dialog is, because the shell never touches a window.</summary>
public partial class MainWindow
{
    private static SavedSearchItemViewModel? SavedSearchOf(object sender) =>
        (sender as FrameworkElement)?.DataContext as SavedSearchItemViewModel;

    private void SaveGlobalSearch_Click(object sender, RoutedEventArgs e) => SaveActiveSearch();

    /// <summary>Saves what the active tab is searching for, from either box — the seed decides the
    /// default scope from which one it came from.</summary>
    private void SaveActiveSearch()
    {
        var saved = _shell.SavedSearches;
        var seed = saved.SeedFor(_shell.ActiveTab);
        if (SavedSearchDialog.Show(this, seed, n => saved.IsNameTaken(n)) is { } result)
            _ = _shell.SaveSearchAsync(result, previousName: null);
    }

    private void SavedSearchRow_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if (SavedSearchOf(sender) is not { } item) return;

        // Middle-click opens a tab of its own, as a folder does elsewhere.
        if (e.ChangedButton == MouseButton.Middle)
        {
            _ = _shell.RunSavedSearchAsync(item, inNewTab: true);
            e.Handled = true;
            return;
        }
        if (e.ChangedButton == MouseButton.Left)
        {
            _ = _shell.RunSavedSearchAsync(item);
            e.Handled = true;
        }
    }

    private void SavedSearchRun_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RunSavedSearchAsync(SavedSearchOf(sender));

    private void SavedSearchRunInNewTab_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RunSavedSearchAsync(SavedSearchOf(sender), inNewTab: true);

    private void SavedSearchEdit_Click(object sender, RoutedEventArgs e)
    {
        if (SavedSearchOf(sender) is { } item) EditSavedSearch(item);
    }

    /// <summary>The one edit, for the sidebar's menu and the Settings page alike.</summary>
    private void EditSavedSearch(SavedSearchItemViewModel item)
    {
        var saved = _shell.SavedSearches;
        var seed = saved.SeedFor(item, _shell.ActiveTab.CurrentPath);
        if (SavedSearchDialog.Show(this, seed, n => saved.IsNameTaken(n, except: item.Name), editingName: item.Name)
            is { } result)
        {
            _ = _shell.SaveSearchAsync(result, previousName: item.Name);
        }
    }

    private void SavedSearchDelete_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RemoveSavedSearchAsync(SavedSearchOf(sender));

    // --- The title bar's dropdown ---

    /// <summary>Opens the saved-search menu under the title bar's button. A menu built on each open,
    /// for the reason the workspace one is: running the search already on show has to run it
    /// again, which a combo box would swallow.</summary>
    private void SavedSearchSwitcher_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu
        {
            PlacementTarget = SavedSearchSwitcher,
            Placement = PlacementMode.Bottom,
            VerticalOffset = 2,
        };
        foreach (var item in BuildSavedSearchMenuItems())
            menu.Items.Add(item);
        menu.IsOpen = true;
    }

    /// <summary>The dropdown's entries: every saved search with its scope beside it, then Save and
    /// Manage. Internal so the harness can photograph the same list.</summary>
    internal List<FrameworkElement> BuildSavedSearchMenuItems()
    {
        var items = new List<FrameworkElement>();

        foreach (var search in _shell.SavedSearches.Items)
        {
            var item = new MenuItem
            {
                // "__" so underscores in names render instead of becoming access keys.
                Header = search.Name.Replace("_", "__"),
                InputGestureText = search.ScopeColumnText,
                ToolTip = search.ToolTip,
            };
            item.Click += (_, _) => _ = _shell.RunSavedSearchAsync(search);
            items.Add(item);
        }

        if (items.Count == 0)
            items.Add(new MenuItem { Header = "No saved searches yet", IsEnabled = false });

        items.Add(new Separator());

        // Only with something to save: the dialog would refuse an empty query anyway, and a greyed
        // entry says why nothing happens better than a dialog that opens to say it.
        var save = new MenuItem
        {
            Header = "Save current search…",
            Icon = MenuIcon("Icon.Save"),
            IsEnabled = !string.IsNullOrWhiteSpace(_shell.ActiveTab.ActiveSearchText),
        };
        save.Click += (_, _) => SaveActiveSearch();
        items.Add(save);

        var manage = new MenuItem { Header = "Manage saved searches…", Icon = MenuIcon("Icon.Settings") };
        manage.Click += (_, _) => ShowSettings(SettingsCategory.SavedSearches);
        items.Add(manage);

        return items;
    }
}
