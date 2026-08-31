namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// The container a suffix names, in this app's own words rather than the library's.
/// </summary>
/// <remarks>
/// It exists so <see cref="ArchiveFormats"/> can say what a name claims to be without referencing
/// SharpCompress, which would spread the dependency past the one class that is allowed to know
/// about it. The reader maps the library's own detection onto this and compares.
/// </remarks>
public enum ArchiveContainer
{
    Zip,
    SevenZip,
    Rar,
    Tar,
    GZip,
}

/// <summary>What kind of container a suffix names, and what can be done with it.</summary>
/// <param name="Suffix">Lowercase, leading dot, possibly compound (<c>.tar.gz</c>).</param>
/// <param name="Container">What a file with this suffix must turn out to be. See the class remarks.</param>
/// <param name="RandomAccess">
/// Whether entries can be addressed individually rather than by reading from the start. Decides
/// which of the library's two APIs the reader uses, and is a <b>hint</b> for menu enablement — never
/// the authority on cost. A solid 7z carries <c>.7z</c> and is sequential in practice, and only the
/// opened container knows that.
/// </param>
/// <param name="Writable">Whether this app can produce or rewrite one.</param>
/// <param name="PreviewByListing">
/// Whether the preview pane may list it on selection. False for the sequential formats: listing a
/// <c>.tar.gz</c> means decompressing the whole stream, and the pane runs on arrow keys.
/// </param>
public sealed record ArchiveFormat(
    string Suffix,
    ArchiveContainer Container,
    bool RandomAccess,
    bool Writable,
    bool PreviewByListing);

/// <summary>
/// The one table of archive suffixes. The preview classifier, the path parser, the navigation gate,
/// the context menu and the harness all read it, so none of them can disagree about what an archive
/// is.
/// </summary>
/// <remarks>
/// <para>
/// <b>Matching is by longest suffix, not <c>Path.GetExtension</c>.</b> <c>backup.tar.gz</c> has to
/// match <c>.tar.gz</c>: matching <c>.gz</c> gives a container holding one unnamed entry, and
/// matching <c>.tar.gz</c> gives the files, which is what the user means. Measured, not assumed —
/// the library really does report a <c>.tar.gz</c> as a GZip holding one entry whose key is null
/// when it is opened as the wrong thing.
/// </para>
/// <para>
/// <b>Office and OpenDocument documents are deliberately absent.</b> They are zip containers, but
/// the shell makes a real page-one thumbnail of them, which beats a listing of their guts every
/// time — and browsing into a <c>.docx</c> as a folder is a novelty, not a feature.
/// </para>
/// <para>
/// <b>So are the standalone single-stream compressors other than <c>.gz</c></b> — <c>.bz2</c>,
/// <c>.xz</c>, <c>.lz</c>, <c>.zst</c>. Nothing in the library names them as containers, so there is
/// no detected type to check a claim against, and browsing into one would show a single unnamed
/// entry. They stay ordinary files. <c>.gz</c> is kept because it really does carry the inner
/// name, so <c>syslog.gz</c> browses to <c>syslog</c>.
/// </para>
/// <para>
/// <b>Writable is narrower than readable, and that is the library's limit rather than a choice.</b>
/// SharpCompress 1.0.0 ships no 7z writer at all — the published documentation describes an
/// unreleased version that does — so 7z is read-only here and the create dialog says so by name
/// instead of greying a row with no explanation.
/// </para>
/// </remarks>
public static class ArchiveFormats
{
    private static readonly ArchiveFormat[] All =
    [
        // Compound tar containers. Sequential by construction: the whole stream decompresses, so
        // these must go through the reader API — opened as an archive, a .tar.gz comes back as a
        // GZip holding one nameless entry and a .tar.bz2 throws outright.
        new(".tar.gz",  ArchiveContainer.Tar, RandomAccess: false, Writable: true,  PreviewByListing: false),
        new(".tar.bz2", ArchiveContainer.Tar, RandomAccess: false, Writable: true,  PreviewByListing: false),
        new(".tar.xz",  ArchiveContainer.Tar, RandomAccess: false, Writable: false, PreviewByListing: false),
        new(".tar.lz",  ArchiveContainer.Tar, RandomAccess: false, Writable: false, PreviewByListing: false),
        new(".tar.zst", ArchiveContainer.Tar, RandomAccess: false, Writable: false, PreviewByListing: false),
        new(".tgz",     ArchiveContainer.Tar, RandomAccess: false, Writable: true,  PreviewByListing: false),
        new(".tbz2",    ArchiveContainer.Tar, RandomAccess: false, Writable: true,  PreviewByListing: false),
        new(".txz",     ArchiveContainer.Tar, RandomAccess: false, Writable: false, PreviewByListing: false),

        // The zip family: a central directory, so entries are addressable.
        new(".zip",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".zipx",    ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".nupkg",   ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".snupkg",  ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".jar",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".war",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".aar",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".vsix",    ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".apk",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".whl",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".crx",     ArchiveContainer.Zip, RandomAccess: true,  Writable: false, PreviewByListing: true),
        new(".epub",    ArchiveContainer.Zip, RandomAccess: true,  Writable: false, PreviewByListing: true),
        new(".xpi",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),
        new(".oxt",     ArchiveContainer.Zip, RandomAccess: true,  Writable: true,  PreviewByListing: true),

        // Read-only formats carrying an index of their own.
        new(".7z",      ArchiveContainer.SevenZip, RandomAccess: true, Writable: false, PreviewByListing: true),
        new(".rar",     ArchiveContainer.Rar,      RandomAccess: true, Writable: false, PreviewByListing: true),

        // Plain tar: addressable, though only by walking every header.
        new(".tar",     ArchiveContainer.Tar,  RandomAccess: true,  Writable: true,  PreviewByListing: true),

        // One compressed file, which carries the inner name.
        new(".gz",      ArchiveContainer.GZip, RandomAccess: false, Writable: true,  PreviewByListing: false),
    ];

    /// <summary>The longest suffix of <paramref name="name"/> that names an archive, or null.</summary>
    public static ArchiveFormat? Match(string? name)
    {
        if (string.IsNullOrEmpty(name)) return null;

        ArchiveFormat? best = null;
        foreach (var format in All)
        {
            if (!name.EndsWith(format.Suffix, StringComparison.OrdinalIgnoreCase)) continue;
            // A name that is only the suffix (".zip") is a dotfile, not an archive.
            if (name.Length == format.Suffix.Length) continue;
            if (best is null || format.Suffix.Length > best.Suffix.Length) best = format;
        }
        return best;
    }

    /// <summary>Whether the name looks like something this app can browse into.</summary>
    public static bool IsArchiveName(string? name) => Match(name) is not null;

    /// <summary>Every suffix, for tests and the syntax card.</summary>
    public static IReadOnlyList<ArchiveFormat> Known => All;
}
