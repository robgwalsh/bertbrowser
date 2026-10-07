namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Reads and rewrites the metadata of one file format.
/// </summary>
/// <remarks>
/// <para>
/// <b>A codec never opens a file.</b> It is handed streams, so the share flags, the cloud
/// placeholder refusal and the reparse rule stay with <see cref="ReadOnlyFile"/> and the executor
/// decides where a rewrite lands.
/// </para>
/// <para>
/// <b><see cref="PayloadDigest"/> is the safety argument.</b> It hashes everything that is
/// <em>not</em> metadata — the scan data of a picture, the frames of a song — and the executor
/// refuses to swap a rewrite in unless that hash is the one the original had. A codec bug then
/// costs an error message rather than somebody's photograph.
/// </para>
/// </remarks>
public interface IMetadataCodec
{
    MetadataFamily Family { get; }

    /// <summary>The fields this format has somewhere to put. Its own answer rather than its
    /// family's: two audio containers do not have room for the same things.</summary>
    IReadOnlySet<MetadataField> Fields { get; }

    /// <summary>Whether a cover picture can be embedded.</summary>
    bool HoldsPicture => false;

    /// <exception cref="MetadataFormatException">The stream is not this format.</exception>
    MetadataDocument Read(Stream source);

    /// <summary>
    /// Writes <paramref name="source"/> to <paramref name="destination"/> with
    /// <paramref name="edit"/> applied. Nothing but metadata may differ between the two.
    /// </summary>
    /// <param name="destination">Empty, readable, writable and seekable.</param>
    /// <exception cref="MetadataFormatException">The stream is not this format, or the change
    /// does not fit in it.</exception>
    void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct);

    /// <summary>A hash of everything in <paramref name="source"/> that is not metadata.</summary>
    /// <exception cref="MetadataFormatException">The stream is not this format.</exception>
    byte[] PayloadDigest(Stream source);
}

/// <summary>Which codec handles a file. The extension decides, never a sniff of the bytes: a
/// codec that then finds something else inside refuses it by name.</summary>
public static class MetadataCodecs
{
    private static readonly Dictionary<string, Func<string, IMetadataCodec>> ByExtension = Build();

    private static Dictionary<string, Func<string, IMetadataCodec>> Build()
    {
        var map = new Dictionary<string, Func<string, IMetadataCodec>>(StringComparer.OrdinalIgnoreCase)
        {
            [".jpg"] = _ => new JpegCodec(),
            [".jpeg"] = _ => new JpegCodec(),
            [".jpe"] = _ => new JpegCodec(),
            [".jfif"] = _ => new JpegCodec(),
            [".png"] = _ => new PngCodec(),
            [".tif"] = _ => new TiffCodec(),
            [".tiff"] = _ => new TiffCodec(),
            [".webp"] = _ => new WebPCodec(),
            [".epub"] = _ => new EpubCodec(),
        };

        // Word, Excel and PowerPoint, with their macro-enabled and template variants: one
        // container, one properties part.
        foreach (var extension in new[]
                 {
                     ".docx", ".docm", ".dotx", ".dotm", ".xlsx", ".xlsm", ".xltx", ".xltm",
                     ".pptx", ".pptm", ".potx", ".potm", ".ppsx", ".ppsm",
                 })
            map[extension] = _ => new OpenXmlCodec();

        // Exactly the containers the payload check knows how to read — no more, whatever the tag
        // library could manage on its own.
        foreach (var extension in AudioPayload.Extensions)
            map[extension] = e => new AudioTagCodec(e);

        return map;
    }

    /// <summary>The extensions something here can write, lowercase with their dot.</summary>
    public static IReadOnlyCollection<string> Extensions => ByExtension.Keys;

    public static bool Handles(string path) => ByExtension.ContainsKey(Path.GetExtension(path));

    public static IMetadataCodec? For(string path)
    {
        var extension = Path.GetExtension(path);
        return ByExtension.TryGetValue(extension, out var make) ? make(extension.ToLowerInvariant()) : null;
    }
}
