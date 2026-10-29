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
    /// <summary>One of the session's entries as a menu element; <see cref="ContextMenuComposer"/>
    /// decides where it goes.</summary>
    /// <param name="ownerWindow">Asked at click time for the handle any dialog an extension shows
    /// should be owned by.</param>
    /// <param name="report">Where a failure message goes — the status bar.</param>
    public static FrameworkElement Build(
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
