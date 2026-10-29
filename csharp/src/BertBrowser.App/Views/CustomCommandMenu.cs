using System.Windows;
using System.Windows.Controls;
using IconPath = System.Windows.Shapes.Path;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Commands;

namespace BertBrowser.App.Views;

/// <summary>Builds a user-defined command's context menu item. Shared by the folder tree's menu and
/// by every pane's file-list menu; <see cref="ContextMenuComposer"/> decides where it goes.</summary>
internal static class CustomCommandMenu
{
    public static MenuItem Build(
        CustomCommandDefinition definition,
        IReadOnlyList<(string FullPath, bool IsDirectory)> targets,
        Action<CustomCommandDefinition, IReadOnlyList<(string FullPath, bool IsDirectory)>> run)
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
            // One of the user's own commands can be given a shortcut on the Keyboard page.
            InputGestureText = GestureText.For(KeymapService.CustomId(definition.Id)),
        };
        item.Click += (_, _) => run(definition, targets);
        return item;
    }
}
