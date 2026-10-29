using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Media3D;

namespace BertBrowser.App.Views;

/// <summary>Visual/logical tree walks shared by the file list, the folder tree, and the drag
/// controllers. Kept in one place because every pane instantiates its own copy of those controls,
/// and the walks must behave identically in all of them.</summary>
internal static class VisualTreeUtil
{
    /// <summary>Walks up to the nearest ancestor of type <typeparamref name="T"/>, crossing from
    /// the visual tree into the logical one where a node isn't visual (content elements such as
    /// <see cref="System.Windows.Documents.Run"/> inside a template).</summary>
    public static T? FindAncestor<T>(DependencyObject? d) where T : DependencyObject
    {
        while (d is not null and not T)
            d = ParentOf(d);
        return d as T;
    }

    public static DependencyObject? ParentOf(DependencyObject d) =>
        d is Visual or Visual3D ? VisualTreeHelper.GetParent(d) : LogicalTreeHelper.GetParent(d);

    /// <summary>Depth-first search for the first descendant of type <typeparamref name="T"/>.
    /// Returns null before the control's template has been applied.</summary>
    public static T? FindDescendant<T>(DependencyObject root) where T : DependencyObject
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is T match) return match;
            if (FindDescendant<T>(child) is { } nested) return nested;
        }
        return null;
    }

    /// <summary>Like <see cref="FindDescendant{T}"/> but also matches <paramref name="root"/>
    /// itself, which is what a mouse-wheel handler wants when the sender is already the scroller.</summary>
    public static ScrollViewer? FindScrollViewer(DependencyObject root) =>
        root as ScrollViewer ?? FindDescendant<ScrollViewer>(root);
}
