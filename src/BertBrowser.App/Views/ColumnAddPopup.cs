using System.Windows;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.Views;

/// <summary>
/// Shows a <see cref="ColumnAddPanel"/> next to whatever opened it.
/// </summary>
/// <remarks>
/// <para>
/// A popup rather than the modal dialog this replaced: adding a column is not a decision that wants
/// an OK button, and a popup can be dismissed by looking away from it. It is used from the settings
/// page's Add button and from the file list header's "More columns…", so both reach the same list
/// and there is no longer a curated set on one path and the property system on another.
/// </para>
/// <para>
/// <see cref="Popup.StaysOpen"/> is false so a click anywhere else closes it, and
/// <c>AllowsTransparency</c> is left off: the panel draws its own opaque border, and a transparent
/// popup would put the list on a layered window that renders its text without ClearType.
/// </para>
/// </remarks>
internal static class ColumnAddPopup
{
    /// <param name="near">What to hang the popup off. The bottom-left of this element, falling back
    /// to above it when there is no room below — <see cref="PlacementMode.Bottom"/>'s own rule.</param>
    /// <param name="read">Reads the layout as it stands, each time the list is rebuilt.</param>
    /// <param name="add">Called with the id of each column clicked. It writes the layout; the panel
    /// then re-reads it through <paramref name="read"/> and the row disappears.</param>
    /// <param name="atMouse">
    /// Open at the pointer instead of under <paramref name="near"/>.
    /// </param>
    /// <remarks>
    /// <paramref name="atMouse"/> is for the header menu, whose "More columns…" sits at the bottom
    /// of a menu most of a window tall: hanging the popup off the header strip put it nowhere near
    /// the pointer that had just clicked. The menu item itself cannot be the placement target —
    /// its own popup is closing by the time this runs — and <see cref="PlacementMode.MousePoint"/>
    /// needs no coordinates from the caller, so there is nothing here to get wrong on a scaled
    /// monitor, and WPF still pulls the popup back inside the screen edge for us.
    /// </remarks>
    public static void Show(
        UIElement near,
        Func<IReadOnlyList<ColumnSetting>?> read,
        Action<string> add,
        bool atMouse = false)
    {
        var panel = new ColumnAddPanel();
        var popup = new Popup
        {
            Child = panel,
            PlacementTarget = near,
            Placement = atMouse ? PlacementMode.MousePoint : PlacementMode.Bottom,
            HorizontalOffset = 0,
            VerticalOffset = atMouse ? 0 : 2,
            StaysOpen = false,
            // Without this the panel's keystrokes go to the window behind it: a Popup is its own
            // top-level HWND, and only a focusable one gets the keyboard.
            Focusable = true,
            AllowsTransparency = false,
        };

        // The panel re-reads the layout itself once this returns, which is what takes the row that
        // was just clicked back out of the list.
        panel.Chosen += add;

        // Escape closes, from the search box or the list — a popup with no Cancel button needs one
        // key that always means "never mind".
        panel.PreviewKeyDown += (_, e) =>
        {
            if (e.Key != Key.Escape) return;
            popup.IsOpen = false;
            e.Handled = true;
        };

        panel.Bind(read);
        popup.IsOpen = true;

        // After IsOpen, not before: the panel has no presentation source until the popup's window
        // exists, and focus set before that lands nowhere.
        popup.Dispatcher.BeginInvoke(() => panel.MoveFocus(
            new TraversalRequest(FocusNavigationDirection.First)));
    }
}
