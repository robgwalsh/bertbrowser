using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>Builds the file-type entries of a "New" submenu. Shared by the folder tree's menu and
/// by every pane's file-list menu, for the reason <see cref="CustomCommandMenu"/> is: the list is
/// the user's and there is one of it.</summary>
internal static class NewItemMenu
{
    /// <summary>Replaces the file-type section of a "New" submenu (everything tagged with a
    /// <see cref="NewFileTemplate"/>) with the types currently configured. "Folder" and "Empty
    /// file…" are declared in XAML either side of <paramref name="anchor"/> and are not touched —
    /// Folder is never configurable, and an empty file is always worth offering.</summary>
    public static void Rebuild(
        MenuItem newMenu,
        Separator anchor,
        AppSettings settings,
        Action<NewFileTemplate> create)
    {
        for (var i = newMenu.Items.Count - 1; i >= 0; i--)
        {
            if (newMenu.Items[i] is MenuItem { Tag: NewFileTemplate })
                newMenu.Items.RemoveAt(i);
        }

        var templates = settings.ResolvedNewFileTypes;
        anchor.Visibility = templates.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        var insertAt = newMenu.Items.IndexOf(anchor) + 1;
        foreach (var template in templates)
        {
            // E7C3 = Page. Font and colour come from resources rather than literals so these
            // runtime-built items match the ones declared in XAML and follow a theme change like
            // everything else.
            var icon = new TextBlock { Text = "", FontSize = 16 };
            icon.SetResourceReference(TextBlock.FontFamilyProperty, "SymbolFont");
            icon.SetResourceReference(TextBlock.ForegroundProperty, ThemeToken.MenuIconForeground);

            // "__" so underscores in names render instead of becoming access keys.
            var item = new MenuItem
            {
                Header = template.Label.Replace("_", "__"),
                InputGestureText = template.Extension,
                Tag = template,
                Icon = icon,
            };
            item.Click += (_, _) => create(template);
            newMenu.Items.Insert(insertAt++, item);
        }
    }
}
