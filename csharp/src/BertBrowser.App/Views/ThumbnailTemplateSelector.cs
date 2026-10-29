using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>In thumbnail mode, media files render as picture tiles while folders and
/// non-media files stay as full-width rows. The sort keeps rows above the tiles, so the
/// list reads as a rows section followed by a thumbnail grid.</summary>
public sealed class ThumbnailTemplateSelector : DataTemplateSelector
{
    public DataTemplate? TileTemplate { get; set; }
    public DataTemplate? RowTemplate { get; set; }

    public override DataTemplate? SelectTemplate(object item, DependencyObject container) =>
        IsTile(item) ? TileTemplate : RowTemplate;

    /// <summary>
    /// Whether an item is drawn as a tile rather than as a full-width row.
    /// </summary>
    /// <remarks>
    /// Public and static because <see cref="VirtualizingWrapPanel"/> has to make the same call: it
    /// works out where every item goes without building any of them, so if it disagreed with the
    /// selector about which shape an item is, the grid would be laid out for one thing and filled
    /// with another.
    /// </remarks>
    public static bool IsTile(object? item) => item is FileItemViewModel { IsMedia: true };
}
