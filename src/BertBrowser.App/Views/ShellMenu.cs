using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Views;

/// <summary>
/// Builds the section of a context menu that belongs to other programs — 7-Zip, Git,
/// TortoiseSVN — from a <see cref="ShellMenuSession"/>. Shared by the folder tree's menu and by
/// every pane's file-list menu, the way <see cref="CustomCommandMenu"/> is.
/// </summary>
/// <remarks>
/// The entries are ordinary <c>MenuItem</c>s under the app's own menu style, not a native menu:
/// they follow the theme, the harness can photograph them detached like any other, and nothing
/// here needs a window handle until an item is clicked. What the shell would have drawn itself —
/// an extension's bitmap beside its item — arrives as an <c>ImageSource</c> and goes where an
/// <c>Icon.*</c> outline would.
/// </remarks>
internal static class ShellMenu
{
    /// <summary>Replaces the shell section of a menu (everything tagged with a
    /// <see cref="ShellMenuEntry"/>) with the session's entries, or removes it when there is no
    /// session.</summary>
    /// <param name="ownerWindow">Asked at click time for the handle any dialog an extension shows
    /// should be owned by.</param>
    /// <param name="report">Where a failure message goes — the status bar.</param>
    public static void Rebuild(
        ContextMenu menu,
        Separator anchor,
        ShellMenuSession? session,
        Func<IntPtr> ownerWindow,
        Action<string> report)
    {
        for (var i = menu.Items.Count - 1; i >= 0; i--)
        {
            if (menu.Items[i] is FrameworkElement { Tag: ShellMenuEntry })
                menu.Items.RemoveAt(i);
        }

        var entries = session?.Entries ?? [];
        anchor.Visibility = entries.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (session is null) return;

        var insertAt = menu.Items.IndexOf(anchor) + 1;
        foreach (var entry in entries)
            menu.Items.Insert(insertAt++, Build(entry, session, ownerWindow, report));
    }

    private static FrameworkElement Build(
        ShellMenuEntry entry, ShellMenuSession session, Func<IntPtr> ownerWindow, Action<string> report)
    {
        if (entry.IsSeparator) return new Separator { Tag = entry };

        // The header is already WPF-ready: ShellMenuRules.Header turned the shell's "&" into "_"
        // and escaped literal underscores.
        var item = new MenuItem
        {
            Header = entry.Header,
            Tag = entry,
            IsEnabled = entry.IsEnabled,
        };

        if (entry.Icon is ImageSource image)
            item.Icon = new Image { Source = image, Width = 16, Height = 16 };

        if (entry.HasChildren)
        {
            foreach (var child in entry.Children)
                item.Items.Add(Build(child, session, ownerWindow, report));
            return item;
        }

        item.Click += (_, _) =>
        {
            if (session.Invoke(entry, ownerWindow()) is { } message) report(message);
        };
        return item;
    }
}
