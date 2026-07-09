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
        item is FileItemViewModel { IsMedia: true } ? TileTemplate : RowTemplate;
}
