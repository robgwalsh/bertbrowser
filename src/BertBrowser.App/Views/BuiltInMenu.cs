using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Views;

/// <summary>
/// The app's own right-click entries as the Context menu page governs them. An item declared in
/// XAML with <c>Tag="id"</c> is one the user can untick, by the ids <see cref="BuiltInMenuItems"/>
/// lists; the separators between groups are then re-decided so a hidden group leaves no doubled
/// or dangling line. Shared by the folder tree's menu and every pane's file-list menu.
/// </summary>
internal static class BuiltInMenu
{
    public static IReadOnlySet<string> Hidden(AppSettings settings) =>
        new HashSet<string>(settings.HiddenBuiltInMenuItems, StringComparer.OrdinalIgnoreCase);

    /// <summary>Shows every tagged top-level item the user has not unticked, and hides the rest.
    /// Runs first, so the few items a right-click hides for its own reasons (Extract on a
    /// non-archive, Settle by content outside a comparison) go through <see cref="Show"/> after.</summary>
    public static void Apply(ContextMenu menu, IReadOnlySet<string> hidden)
    {
        foreach (var item in ContextMenuComposer.BuiltInItems(menu))
            Show(item, show: true, hidden);
    }

    /// <summary>
    /// Prints each entry's shortcut beside it, from the keymap in force. For the file list's menu
    /// only: the folder tree's has always shown none, because the list's keys do not act on the
    /// tree. Run on every opening, so a rebind is in the menu the next time it is opened.
    /// </summary>
    public static void ApplyGestures(ContextMenu menu)
    {
        foreach (var item in ContextMenuComposer.BuiltInItems(menu))
        {
            // An item naming its own command (Extract here, Extract to — two under one id) wins
            // over the row's, and has been filled in by Cmd.Id already.
            if (Cmd.GetId(item) is not null) continue;
            if (item.Tag is string id && BuiltInMenuItems.Find(id)?.CommandId is { } command)
                item.InputGestureText = GestureText.For(command);
        }
    }

    /// <summary>Shows the item when the menu wants it and the user has not unticked it.</summary>
    public static void Show(MenuItem item, bool show, IReadOnlySet<string> hidden)
    {
        var id = item.Tag as string
            ?? throw new InvalidOperationException($"'{item.Header}' is not tagged as one of the app's menu entries.");
        item.Visibility = show && BuiltInMenuItems.IsShown(id, hidden)
            ? Visibility.Visible
            : Visibility.Collapsed;
    }

    /// <summary>Re-decides every top-level separator from what is visible around it. Last, after
    /// the built sections (New, the user's commands, other programs' entries) are in place.</summary>
    public static void TidySeparators(ContextMenu menu)
    {
        var elements = menu.Items.OfType<FrameworkElement>().ToList();
        var slots = elements
            .Select(e => new MenuSlot(e is Separator, e.Visibility == Visibility.Visible))
            .ToList();

        var visible = MenuSeparatorRules.Apply(slots);
        for (var i = 0; i < elements.Count; i++)
        {
            if (elements[i] is Separator separator)
                separator.Visibility = visible[i] ? Visibility.Visible : Visibility.Collapsed;
        }
    }
}
