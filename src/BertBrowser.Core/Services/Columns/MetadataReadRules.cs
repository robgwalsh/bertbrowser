using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Core.Services.Columns;

/// <summary>
/// Whether a metadata column may open a file at all.
/// </summary>
/// <remarks>
/// <para>
/// This is the most dangerous rule in the columns feature, so it lives here rather than inside the
/// interop that enforces it — in Core, where xUnit can hold it still. The read itself is
/// <c>SHGetPropertyStoreFromParsingName</c> with <c>GPS_OPENSLOWITEM</c>, which genuinely opens the
/// file so that EXIF and ID3 handlers run.
/// </para>
/// </remarks>
public static class MetadataReadRules
{
    /// <summary>
    /// Whether the file's properties may be read.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>A cloud placeholder is refused, and that is what this rule is for.</b> Opening one makes
    /// the sync provider fetch its content — so a Dimensions column over a synced photo folder would
    /// download the entire folder, one row at a time, as the user scrolled past it, with no progress
    /// and nothing asked. The preview pane already refuses these for the same reason and
    /// <see cref="PreviewClassifier.IsCloudPlaceholder"/> is the shared predicate, so the two cannot
    /// drift apart.
    /// </para>
    /// <para>
    /// A reparse point is refused rather than followed — <c>ContentReader</c>'s rule. A directory is
    /// skipped because there is nothing there worth the open. Both render blank, which is what
    /// unknown looks like everywhere else in this app.
    /// </para>
    /// </remarks>
    public static bool MayRead(FileAttributes attributes) =>
        !attributes.HasFlag(FileAttributes.Directory) &&
        !attributes.HasFlag(FileAttributes.ReparsePoint) &&
        !PreviewClassifier.IsCloudPlaceholder(attributes);
}
