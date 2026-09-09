using System.Text;

namespace BertBrowser.Core.Services.Checksums;

/// <summary>One listed file and the digest claimed for it.</summary>
/// <param name="Binary">
/// The GNU tools' <c>*</c> marker, meaning the file was read in binary mode. Recorded so a
/// round-trip preserves it; this app always reads binary, so it changes nothing about the answer.
/// </param>
public sealed record ChecksumLine(string Name, string Digest, bool Binary = true);

public enum ChecksumFileProblemKind
{
    /// <summary>Not a checksum line at all, or a digest of a length no algorithm produces.</summary>
    Malformed,

    /// <summary>The name would resolve outside the folder being verified. See <see cref="ChecksumPath"/>.</summary>
    UnsafePath,

    /// <summary>The same name listed twice, with no way to say which digest is meant.</summary>
    DuplicateName,
}

/// <param name="LineNumber">1-based, so it can be quoted back to the user.</param>
public sealed record ChecksumFileProblem(int LineNumber, string Text, ChecksumFileProblemKind Kind);

/// <param name="Algorithm">
/// What the file holds, or null when it could not be established — an unknown extension whose
/// digests disagree about their length.
/// </param>
public sealed record ChecksumFileParse(
    ChecksumAlgorithm? Algorithm,
    IReadOnlyList<ChecksumLine> Lines,
    IReadOnlyList<ChecksumFileProblem> Problems);

/// <summary>
/// Reading and writing the checksum files the rest of the world exchanges.
/// </summary>
/// <remarks>
/// <para>
/// Two shapes, and the difference is the order. The <c>*sum</c> family writes
/// <c>&lt;digest&gt;  &lt;name&gt;</c>; SFV writes <c>&lt;name&gt; &lt;CRC&gt;</c>. Which one applies is decided
/// by the file's <em>extension</em>, not by sniffing the first line — a file called
/// <c>things.sfv</c> is an SFV even if its names happen to look like hex, and guessing would make
/// that file unreadable in a way its author could never diagnose. Sniffing is the fallback for a
/// name that says nothing, like <c>CHECKSUMS.txt</c>.
/// </para>
/// <para>
/// <strong>Problems are collected, never thrown.</strong> One unparseable line must not cost the
/// other three hundred their answer — the same rule every executor in this app follows, and the
/// reason a half-corrupt checksum file still verifies the half that is intact and says so.
/// </para>
/// </remarks>
public static class ChecksumFile
{
    /// <summary>
    /// Parses the text of a checksum file.
    /// </summary>
    /// <param name="fileName">
    /// The file's own name, which is what decides the line order. Null or unrecognised means the
    /// shape is worked out from the content instead.
    /// </param>
    public static ChecksumFileParse Parse(string text, string? fileName)
    {
        var declared = ChecksumAlgorithms.FromExtension(fileName);
        var digestLast = declared is ChecksumAlgorithm.Crc32 || (declared is null && SniffsAsDigestLast(text));

        var lines = new List<ChecksumLine>();
        var problems = new List<ChecksumFileProblem>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var lengths = new HashSet<int>();

        var number = 0;
        foreach (var raw in text.ReplaceLineEndings("\n").Split('\n'))
        {
            number++;

            var line = raw.TrimEnd();
            var trimmed = line.TrimStart();
            if (trimmed.Length == 0) continue;

            // Both comment markers are accepted in every format: SFV writes ';', the *sum family
            // and hand-written lists use '#'. Reading is generous; writing is conventional.
            if (trimmed[0] is ';' or '#') continue;

            var (name, digest, binary) = Split(trimmed, digestLast);
            if (name is null || digest is null)
            {
                problems.Add(new ChecksumFileProblem(number, line, ChecksumFileProblemKind.Malformed));
                continue;
            }

            if (ChecksumAlgorithms.Recognise(digest) is null)
            {
                problems.Add(new ChecksumFileProblem(number, line, ChecksumFileProblemKind.Malformed));
                continue;
            }

            if (!seen.Add(name))
            {
                problems.Add(new ChecksumFileProblem(number, line, ChecksumFileProblemKind.DuplicateName));
                continue;
            }

            lengths.Add(digest.Length);
            lines.Add(new ChecksumLine(name, digest, binary));
        }

        // The extension is the label; the digests are the fact. When they disagree the fact wins,
        // and when the digests disagree among themselves there is no single answer to give.
        var algorithm = declared;
        if (lengths.Count == 1)
        {
            var byLength = ChecksumAlgorithms.FromDigestLength(lengths.Single());
            if (byLength is not null) algorithm = byLength;
        }
        else if (lengths.Count > 1)
        {
            algorithm = null;
        }

        return new ChecksumFileParse(algorithm, lines, problems);
    }

    /// <summary>
    /// Renders a checksum file. Casing follows <see cref="ChecksumAlgorithms.IsWrittenLowercase"/>,
    /// which is the only place in the app that decides it.
    /// </summary>
    public static string Render(ChecksumAlgorithm algorithm, IEnumerable<ChecksumLine> lines)
    {
        var sfv = algorithm is ChecksumAlgorithm.Crc32;
        var lower = ChecksumAlgorithms.IsWrittenLowercase(algorithm);

        var text = new StringBuilder();

        // SFV's own convention. The *sum family carries no header, and adding one would make the
        // file differ from what sha256sum produces for no gain.
        if (sfv) text.Append("; Generated by BertBrowser\n");

        foreach (var line in lines)
        {
            var digest = lower ? line.Digest.ToLowerInvariant() : line.Digest.ToUpperInvariant();

            if (sfv) text.Append(line.Name).Append(' ').Append(digest).Append('\n');
            else text.Append(digest).Append(line.Binary ? "  *" : "  ").Append(line.Name).Append('\n');
        }

        return text.ToString();
    }

    /// <summary>
    /// Whether an unlabelled file looks like it puts the digest last.
    /// </summary>
    /// <remarks>
    /// Only consulted when the extension says nothing. A first line whose <em>first</em> token is a
    /// well-formed digest is the ordinary form; otherwise, if its last token is, it is SFV-shaped.
    /// </remarks>
    private static bool SniffsAsDigestLast(string text)
    {
        foreach (var raw in text.ReplaceLineEndings("\n").Split('\n'))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line[0] is ';' or '#') continue;

            var tokens = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            if (tokens.Length < 2) continue;

            if (ChecksumAlgorithms.Recognise(tokens[0].TrimStart('*')) is not null) return false;
            return ChecksumAlgorithms.Recognise(tokens[^1]) is not null;
        }

        return false;
    }

    /// <summary>Splits one line into a name and a digest, per the format's order.</summary>
    private static (string? Name, string? Digest, bool Binary) Split(string line, bool digestLast)
    {
        if (digestLast)
        {
            // "some file name.zip A1B2C3D4" — the name may hold spaces, so cut at the last run.
            var cut = line.LastIndexOfAny(Whitespace);
            if (cut <= 0) return (null, null, false);

            return (line[..cut].TrimEnd(), line[(cut + 1)..], true);
        }

        // "<digest>  <name>" or "<digest> *<name>" — the digest never holds a space, so cut at the
        // first run and let the name keep whatever it contains.
        var end = line.IndexOfAny(Whitespace);
        if (end <= 0 || end == line.Length - 1) return (null, null, false);

        var digest = line[..end];
        var rest = line[(end + 1)..].TrimStart();
        if (rest.Length == 0) return (null, null, false);

        var binary = rest[0] == '*';
        return (binary ? rest[1..] : rest, digest, binary);
    }

    private static readonly char[] Whitespace = [' ', '\t'];
}
