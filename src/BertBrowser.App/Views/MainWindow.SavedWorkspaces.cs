using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using IconPath = System.Windows.Shapes.Path;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.SavedWorkspaces;

namespace BertBrowser.App.Views;

/// <summary>The workspace switcher — the sidebar's Workspaces section or the title bar's dropdown,
/// whichever Settings picked — and the rename and delete the Settings page asks for. Each handler
/// is a line into the shell, which owns what saving, switching, renaming and deleting one means.
/// The dialog is shown here, as every dialog is, because the shell never touches a window.</summary>
public partial class MainWindow
{
    private static SavedWorkspaceItemViewModel? WorkspaceOf(object sender) =>
        (sender as FrameworkElement)?.DataContext as SavedWorkspaceItemViewModel;

    private void SaveWorkspace_Click(object sender, RoutedEventArgs e) => SaveWorkspace();

    private void SaveWorkspace()
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
        if (WorkspaceOf(sender) is { } item) RenameWorkspace(item);
    }

    private void WorkspaceDelete_Click(object sender, RoutedEventArgs e) =>
        _ = _shell.RemoveWorkspaceAsync(WorkspaceOf(sender));

    /// <summary>The one rename, for the sidebar's menu and the Settings page alike.</summary>
    private void RenameWorkspace(SavedWorkspaceItemViewModel item)
    {
        var workspaces = _shell.SavedWorkspaces;
        if (SavedWorkspaceDialog.Show(this, item.Name, n => workspaces.IsNameTaken(n, except: item.Name), editingName: item.Name)
            is { } name)
        {
            _ = _shell.RenameWorkspaceAsync(item, name);
        }
    }

    // --- The title bar's dropdown ---

    /// <summary>
    /// Opens the workspace menu under the title bar's button.
    /// </summary>
    /// <remarks>
    /// A menu built on each open rather than a <see cref="ComboBox"/>: choosing the workspace the
    /// window already came from has to switch back to it — undoing whatever was opened since — and a
    /// combo box raises nothing when its current item is picked again. It also leaves room for the
    /// two actions under the list, which a combo box has nowhere to put.
    /// </remarks>
    private void WorkspaceSwitcher_Click(object sender, RoutedEventArgs e)
    {
        var menu = new ContextMenu
        {
            PlacementTarget = WorkspaceSwitcher,
            Placement = PlacementMode.Bottom,
            // The button's left edge, lined up under the field rather than under its padding.
            HorizontalOffset = 0,
            VerticalOffset = 2,
        };
        foreach (var item in BuildWorkspaceMenuItems())
            menu.Items.Add(item);
        menu.IsOpen = true;
    }

    /// <summary>The dropdown's entries: every workspace, ticked if it is the one the window came
    /// from, then Save and Manage. Internal so the harness can photograph the same list.</summary>
    internal List<FrameworkElement> BuildWorkspaceMenuItems()
    {
        var items = new List<FrameworkElement>();

        foreach (var workspace in _shell.SavedWorkspaces.Items)
        {
            var item = new MenuItem
            {
                // "__" so underscores in names render instead of becoming access keys.
                Header = workspace.Name.Replace("_", "__"),
                InputGestureText = workspace.ShapeText,
                IsChecked = string.Equals(workspace.Name, _shell.CurrentWorkspaceName, StringComparison.OrdinalIgnoreCase),
                ToolTip = workspace.FoldersToolTip,
            };
            item.Click += (_, _) => _ = _shell.SwitchWorkspaceAsync(workspace);
            items.Add(item);
        }

        if (items.Count == 0)
            items.Add(new MenuItem { Header = "No saved workspaces yet", IsEnabled = false });

        items.Add(new Separator());

        var save = new MenuItem { Header = "Save current layout…", Icon = MenuIcon("Icon.Save") };
        save.Click += (_, _) => SaveWorkspace();
        items.Add(save);

        var manage = new MenuItem { Header = "Manage workspaces…", Icon = MenuIcon("Icon.Settings") };
        manage.Click += (_, _) => ShowSettings(SettingsCategory.Workspaces);
        items.Add(manage);

        return items;
    }

    /// <summary>A resource-referenced outline, so a runtime-built item follows a theme change the
    /// way the ones declared in XAML do.</summary>
    private static IconPath MenuIcon(string key)
    {
        var icon = new IconPath();
        icon.SetResourceReference(StyleProperty, "MenuIconPath");
        icon.SetResourceReference(IconPath.DataProperty, key);
        return icon;
    }
}
