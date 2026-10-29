using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// One row of a folder breakdown: a child, what it weighs, and how much of the parent that is.
/// </summary>
/// <remarks>
/// The bar is driven by <see cref="Fraction"/> rather than by the raw byte count so the view never
/// has to do the arithmetic, and an unknown child gets a fraction of zero — no bar at all. Drawing
/// a bar for something nobody measured would claim a share it has not been shown to have.
/// </remarks>
public sealed partial class DiskUsageTileViewModel : ObservableObject
{
    public DiskUsageTileViewModel(DiskUsageNode node, long largestSiblingBytes)
    {
        Node = node;
        Fraction = largestSiblingBytes > 0 && node.SizeBytes is { } bytes
            ? Math.Clamp((double)bytes / largestSiblingBytes, 0, 1)
            : 0;
    }

    public DiskUsageNode Node { get; }

    public string Name => Node.Name;
    public string FullPath => Node.DisplayPath;
    public bool IsDirectory => Node.IsDirectory;
    public bool IsUnknown => Node.SizeBytes is null;

    /// <summary>This child's share of the largest sibling, which is what the bar is scaled to —
    /// so the biggest item fills the row and everything else reads against it.</summary>
    public double Fraction { get; }

    /// <summary>
    /// The size, or "Not measured" — never a zero standing in for an absent measurement. A folder
    /// on a volume the indexer never reached has no number, and saying "0 B" would claim it is
    /// empty. The trailing <c>*</c> for a partial total is the same mark the file list uses.
    /// </summary>
    public string SizeText =>
        Node.SizeBytes is { } bytes
            ? ByteSizeFormatter.Format(bytes) + (Node.Incomplete ? " *" : "")
            : "Not measured";

    /// <summary>A synthetic row, so the two of them can be styled apart from real children.</summary>
    public bool IsSynthetic { get; init; }
}
