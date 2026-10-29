namespace BertBrowser.Core.Services.Archives;

/// <summary>A container this app can produce.</summary>
public enum ArchiveWriteFormat
{
    Zip,
    Tar,
    TarGz,
    TarBz2,
}

/// <summary>How hard to try, in the three words a person actually means.</summary>
public enum CompressionLevel
{
    /// <summary>No compression. The right answer for a folder of JPEGs or MP4s.</summary>
    Store,
    Normal,
    Maximum,
}

/// <summary>
/// What each write format is called, what it produces, and what this app will not produce at all.
/// </summary>
/// <remarks>
/// <b>7z and RAR are refused by name rather than greyed out with no explanation.</b> SharpCompress
/// 1.0.0 ships no 7z writer at all — its published documentation describes an unreleased version
/// that does, which is exactly the kind of thing to state once and pin with a test — and nothing but
/// WinRAR writes RAR. Both remain perfectly readable; it is only creating them that is impossible.
/// </remarks>
public static class ArchiveWriteRules
{
    public sealed record WriteFormatInfo(
        ArchiveWriteFormat Format, string Label, string Suffix, bool SupportsLevel);

    private static readonly WriteFormatInfo[] All =
    [
        new(ArchiveWriteFormat.Zip,    "Zip",     ".zip",     SupportsLevel: true),
        new(ArchiveWriteFormat.Tar,    "Tar",     ".tar",     SupportsLevel: false),
        new(ArchiveWriteFormat.TarGz,  "Tar.gz",  ".tar.gz",  SupportsLevel: false),
        new(ArchiveWriteFormat.TarBz2, "Tar.bz2", ".tar.bz2", SupportsLevel: false),
    ];

    public static IReadOnlyList<WriteFormatInfo> Formats => All;

    public static WriteFormatInfo Info(ArchiveWriteFormat format) =>
        All.First(f => f.Format == format);

    public static string SuffixFor(ArchiveWriteFormat format) => Info(format).Suffix;

    /// <summary>
    /// Why a format cannot be created, or null when it can. Read by the dialog so the message is
    /// the reason rather than a disabled control.
    /// </summary>
    public static string? WhyNotWritable(string? suffix)
    {
        if (suffix is null) return null;

        return suffix.ToLowerInvariant() switch
        {
            ".7z" => "7z archives can be read but not created — no managed 7z writer exists.",
            ".rar" => "RAR archives can be read but not created — only WinRAR can write them.",
            ".tar.xz" or ".txz" or ".tar.zst" or ".tar.lz" =>
                $"{suffix} archives can be read but not created.",
            _ => null,
        };
    }

    /// <summary>
    /// The name a new archive gets from what is going into it: the folder's own name for a single
    /// folder, the file's stem for a single file, and the containing folder's name otherwise.
    /// </summary>
    public static string SuggestName(IReadOnlyList<string> sources, string currentDirectory)
    {
        if (sources.Count == 1)
        {
            var only = sources[0];
            var name = Path.GetFileName(only.TrimEnd('\\'));
            if (name.Length > 0)
                return Directory.Exists(only) ? name : Path.GetFileNameWithoutExtension(name);
        }

        var folder = Path.GetFileName(currentDirectory.TrimEnd('\\'));
        return folder.Length > 0 ? folder : "Archive";
    }
}
