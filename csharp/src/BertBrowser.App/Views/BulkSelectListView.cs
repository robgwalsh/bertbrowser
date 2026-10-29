using System.Collections;
using System.Windows.Controls;

namespace BertBrowser.App.Views;

/// <summary>
/// A <see cref="ListView"/> whose selection can be replaced in one go.
/// </summary>
/// <remarks>
/// <c>SelectedItems.Add</c> is a whole selection change of its own — begun, resolved against the
/// items and ended, with a <c>SelectionChanged</c> raised — so choosing ten thousand rows through
/// it is ten thousand of them, and measured at eight to nine seconds where Select all, which is
/// one, takes forty milliseconds. <see cref="ListBox.SetSelectedItems"/> is the one-change form
/// for an arbitrary set of rows, and it is protected; this exists to reach it.
/// </remarks>
public sealed class BulkSelectListView : ListView
{
    /// <summary>Selects exactly <paramref name="items"/>, raising <c>SelectionChanged</c> once.</summary>
    public void SelectOnly(IEnumerable items) => SetSelectedItems(items);
}
