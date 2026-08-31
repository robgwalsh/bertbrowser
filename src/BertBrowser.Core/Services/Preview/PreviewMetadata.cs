namespace BertBrowser.Core.Services.Preview;

/// <summary>One property as the Windows property system handed it over: the canonical key, the
/// localised label, and the display-formatted value.</summary>
public sealed record ShellPropertyRow(string Canonical, string Name, string Value);

/// <summary>One row of the preview pane's details strip.</summary>
public sealed record MetadataRow(string Label, string Value);

/// <summary>
/// Picks the handful of properties worth putting under a preview, and puts them in a useful order.
/// </summary>
/// <remarks>
/// Selected by <em>canonical</em> name — <c>System.Image.Dimensions</c>, never "Dimensions". The
/// property store's display names are localised, so matching on them works perfectly on the machine
/// it was written on and silently returns nothing on a German or Japanese Windows. The label shown
/// is still the localised one; only the choosing is done on the invariant key.
///
/// The order is the point as much as the selection: the Properties dialog already exists for the
/// exhaustive dump, so this is the opposite — what a person wants at a glance, first.
/// </remarks>
public static class PreviewMetadata
{
    /// <summary>Rows past this are dropped. The strip sits under the preview, not instead of it.</summary>
    public const int MaxRows = 12;

    public static IReadOnlyList<MetadataRow> Select(PreviewKind kind, IReadOnlyList<ShellPropertyRow> properties)
    {
        var wanted = OrderFor(kind);
        if (wanted.Length == 0 || properties.Count == 0) return [];

        var byCanonical = new Dictionary<string, ShellPropertyRow>(StringComparer.OrdinalIgnoreCase);
        foreach (var property in properties)
        {
            if (property.Canonical.Length == 0 || property.Value.Length == 0) continue;
            byCanonical.TryAdd(property.Canonical, property);
        }

        var rows = new List<MetadataRow>(Math.Min(wanted.Length, MaxRows));
        foreach (var canonical in wanted)
        {
            if (rows.Count == MaxRows) break;
            if (!byCanonical.TryGetValue(canonical, out var property)) continue;

            // A property with no registered display name still has a canonical one; showing the
            // tail of the key beats showing a blank label.
            var label = property.Name.Length > 0 ? property.Name : ShortenCanonical(canonical);
            rows.Add(new MetadataRow(label, property.Value));
        }
        return rows;
    }

    /// <summary>"System.Image.Dimensions" -> "Dimensions". The last-resort header for a property
    /// with no registered display name, shared with the column catalogue so the two cannot differ.</summary>
    internal static string ShortenCanonical(string canonical)
    {
        var dot = canonical.LastIndexOf('.');
        return dot >= 0 && dot < canonical.Length - 1 ? canonical[(dot + 1)..] : canonical;
    }

    private static string[] OrderFor(PreviewKind kind) => kind switch
    {
        PreviewKind.Image => ImageOrder,
        PreviewKind.Media => MediaOrder,
        PreviewKind.Document => DocumentOrder,
        // Text, archives and fonts have better numbers of their own — the encoding, the entry
        // count, the family name — and the shell has nothing to add to them.
        _ => [],
    };

    /// <summary>Also the seed for the column catalogue's curated shell columns — one copy of these
    /// canonical names, because a second would drift with nothing to notice.</summary>
    internal static readonly string[] ImageOrder =
    [
        "System.Image.Dimensions",
        "System.Image.BitDepth",
        "System.Image.ColorSpace",
        "System.Photo.DateTaken",
        "System.Photo.CameraManufacturer",
        "System.Photo.CameraModel",
        "System.Photo.LensModel",
        "System.Photo.FNumber",
        "System.Photo.ExposureTime",
        "System.Photo.ISOSpeed",
        "System.Photo.FocalLength",
        "System.GPS.Latitude",
        "System.GPS.Longitude",
    ];

    /// <summary>Also the seed for the column catalogue's curated shell columns — one copy of these
    /// canonical names, because a second would drift with nothing to notice.</summary>
    internal static readonly string[] MediaOrder =
    [
        "System.Title",
        "System.Music.Artist",
        "System.Music.AlbumTitle",
        "System.Music.TrackNumber",
        "System.Music.Genre",
        "System.Media.Year",
        "System.Media.Duration",
        "System.Video.FrameWidth",
        "System.Video.FrameHeight",
        "System.Video.FrameRate",
        "System.Video.Compression",
        "System.Video.EncodingBitrate",
        "System.Audio.Format",
        "System.Audio.EncodingBitrate",
        "System.Audio.SampleRate",
        "System.Audio.ChannelCount",
    ];

    /// <summary>Also the seed for the column catalogue's curated shell columns — one copy of these
    /// canonical names, because a second would drift with nothing to notice.</summary>
    internal static readonly string[] DocumentOrder =
    [
        "System.Title",
        "System.Author",
        "System.Subject",
        "System.Document.PageCount",
        "System.Document.WordCount",
        "System.Company",
        "System.ApplicationName",
        "System.Document.DateCreated",
        "System.Document.DateSaved",
    ];
}
