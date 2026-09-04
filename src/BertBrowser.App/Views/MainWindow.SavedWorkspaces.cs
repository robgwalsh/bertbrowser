using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.App.Views;

/// <summary>The sidebar's Workspaces section and its header save button: each handler is a line
/// into the shell, which owns what saving, switching, renaming and deleting one means. The dialog
/// is shown here, as every dialog is, because the shell never touches a window.</summary>
public partial class MainWindow
{
    private static SavedWorkspaceItemViewModel? WorkspaceOf(object sender) =>
        (sender as FrameworkElement)?.DataContext as SavedWorkspaceItemViewModel;

    private void SaveWorkspace_Click(object sender, RoutedEventArgs e)
    {
        var workspaces = _shell.SavedWorkspaces;
        var seedName = SavedWorkspaceRules.DefaultName(DateTime.Now);
        if (SavedWorkspaceDialog.Show(this, seedName, n => workspaces.IsNameTaken(n)) is { } name)
            _ = _shell.SaveWorkspaceAsync(new SavedWorkspace(name, _shell.CaptureLayout()), previousName: null);
    }

    private void WorkspaceRow_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if (WorkspaceOf(sender) is not { } item) return;
        if (e.ChangedButton != MouseButton.Left) return;

        _ = _shell.SwitchWorkspaceAsync(item);
        e.Handled = true;
    }

    private void WorkspaceSwitch_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.SwitchWorkspaceAsync(WorkspaceOf(sender));

    private void WorkspaceRename_Click(object sender, RoutedEventArgs e)
    {
        if (WorkspaceOf(sender) is not { } item) return;

        var workspaces = _shell.SavedWorkspaces;
        if (SavedWorkspaceDialog.Show(this, item.Name, n => workspaces.IsNameTaken(n, except: item.Name), editingName: item.Name)
            is { } name)
        {
            _ = _shell.SaveWorkspaceAsync(new SavedWorkspace(name, item.Model.Layout), previousName: item.Name);
        }
    }

    private void WorkspaceDelete_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RemoveWorkspaceAsync(WorkspaceOf(sender));
}
