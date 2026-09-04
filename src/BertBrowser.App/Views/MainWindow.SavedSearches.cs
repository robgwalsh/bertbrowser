using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>The sidebar's Saved searches section: each handler is a line into the shell, which
/// owns what running, editing and deleting one means.</summary>
public partial class MainWindow
{
    private static SavedSearchItemViewModel? SavedSearchOf(object sender) =>
        (sender as FrameworkElement)?.DataContext as SavedSearchItemViewModel;

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

    private void SavedSearchDelete_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RemoveSavedSearchAsync(SavedSearchOf(sender));
}
