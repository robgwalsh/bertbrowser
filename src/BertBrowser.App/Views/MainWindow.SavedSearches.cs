using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>The sidebar's Saved searches section and the header box's save button: each handler
/// is a line into the shell, which owns what running, saving and deleting one means. The dialog
/// is shown here, as every dialog is, because the shell never touches a window.</summary>
public partial class MainWindow
{
    private static SavedSearchItemViewModel? SavedSearchOf(object sender) =>
        (sender as FrameworkElement)?.DataContext as SavedSearchItemViewModel;

    private void SaveGlobalSearch_Click(object sender, RoutedEventArgs e)
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
        if (SavedSearchOf(sender) is not { } item) return;

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
}
