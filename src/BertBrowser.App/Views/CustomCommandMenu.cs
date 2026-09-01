using System.Windows;
using System.Windows.Controls;
using IconPath = System.Windows.Shapes.Path;
using BertBrowser.App.Services;

namespace BertBrowser.App.Views;

/// <summary>Builds the user-defined command section of a context menu. Shared by the folder tree's
/// menu and by every pane's file-list menu.</summary>
internal static class CustomCommandMenu
{
    /// <summary>Replaces the custom-command section of a context menu (everything tagged with a
    /// <see cref="CustomCommandDefinition"/>) with the entries applicable to the given targets.</summary>
    public static void Rebuild(
        ContextMenu menu,
        Separator anchor,
        IReadOnlyList<(string FullPath, bool IsDirectory)> targets,
        AppSettings settings,
        Action<CustomCommandDefinition, IReadOnlyList<(string FullPath, bool IsDirectory)>> run)
    {
        for (var i = menu.Items.Count - 1; i >= 0; i--)
        {
            if (menu.Items[i] is MenuItem { Tag: CustomCommandDefinition })
                menu.Items.RemoveAt(i);
        }

        var applicable = settings.CustomCommands
            .Where(c => targets.Any(t => t.IsDirectory ? c.AppliesToDirectories : c.AppliesToFiles))
            .ToList();
        anchor.Visibility = applicable.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        var insertAt = menu.Items.IndexOf(anchor) + 1;
        foreach (var definition in applicable)
        {
            // A shield when the command will ask for administrator rights, so the menu says so
            // rather than only the prompt. Style and outline come from resources rather than
            // literals, so these runtime-built items are the same thing as the ones declared in
            // XAML and follow a theme change with them. Icon names live in tools/icon/icons.txt.
            var icon = new IconPath();
            icon.SetResourceReference(FrameworkElement.StyleProperty, "MenuIconPath");
            icon.SetResourceReference(
                IconPath.DataProperty, definition.RunElevated ? "Icon.Shield" : "Icon.CustomCommand");

            // "__" so underscores in names render instead of becoming access keys.
            var item = new MenuItem
            {
                Header = definition.Name.Replace("_", "__"),
                Tag = definition,
                Icon = icon,
            };
            item.Click += (_, _) => run(definition, targets);
            menu.Items.Insert(insertAt++, item);
        }
    }
}
