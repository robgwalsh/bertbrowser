using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Services;
using BertBrowser.Core.Theming;

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
            // E8A7 = OpenInNewWindow: reads as "launch externally". EA18 is the shield, so a
            // command that will ask for administrator rights says so in the menu rather than only
            // when the prompt appears. Font and colour come from resources rather than literals so
            // these runtime-built items match the ones declared in XAML and follow a theme change
            // like everything else.
            var icon = new TextBlock
            {
                Text = definition.RunElevated ? "" : "",
                FontSize = 16,
            };
            icon.SetResourceReference(TextBlock.FontFamilyProperty, "SymbolFont");
            icon.SetResourceReference(TextBlock.ForegroundProperty, ThemeToken.MenuIconForeground);

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
