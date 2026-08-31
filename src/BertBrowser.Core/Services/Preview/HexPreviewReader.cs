using System.Text;

namespace BertBrowser.Core.Services.Preview;

/// <summary>One row of a hex dump, already formatted and already coloured.</summary>
/// <param name="Offset">Where in the file this row starts.</param>
/// <param name="Text">The whole row: offset, the hex column, then the ASCII column.</param>
/// <param name="Spans">A complete, gap-free cover of <paramref name="Text"/>. Complete because the
/// view builds one run per span and renders nothing else — a gap does not show as plain text, it
/// deletes the characters under it.</param>
public readonly record struct HexRow(long Offset, string Text, IReadOnlyList<SyntaxSpan> Spans);

/// <summary>What a hex read turned out to be. The footer says all of it.</summary>
/// <param name="Rows">The formatted rows, in order. Empty for an empty file.</param>
/// <param name="BytesShown">How many bytes the rows account for.</param>
/// <param name="Truncated">The file has more than this.</param>
public sealed record HexPreview(IReadOnlyList<HexRow> Rows, long BytesShown, bool Truncated);

/// <summary>
/// Formats the front of a file as a hex dump. Pure over a <see cref="Stream"/> — it never opens
/// anything and never throws, the same contract <see cref="ArchiveListing"/> keeps, which is what
/// lets every rule in it be tested against a <see cref="MemoryStream"/>.
/// </summary>
/// <remarks>
/// Rows carry their own spans rather than the whole dump arriving as one string with one span list.
/// The pane's line splitter could rebase a global list, but a cover is much easier to get wrong
/// across sixty-five thousand rows than within one, and here the cover is a property a single row
/// can be asserted to hold.
///
/// The colouring reuses <see cref="SyntaxClass"/> rather than minting hex-specific theme tokens:
/// the five syntax colours are already contrast-checked against every built-in palette, and a
/// parallel family would be a second set to keep in step for no gain.
/// </remarks>
public static class HexPreviewReader
{
    /// <summary>Bytes per row. Sixteen, split eight and eight, because that is what every other
    /// dump does and the offsets then line up with a person's mental arithmetic.</summary>
    public const int BytesPerRow = 16;

    /// <summary>Rows beyond this are not shown — the analogue of
    /// <see cref="TextPreviewReader.DefaultMaxLines"/>, and needed for the same reason. Hex costs
    /// about four characters per byte, so the pane's megabyte budget would otherwise be 65,536
    /// paragraphs in a flow document.</summary>
    public const int DefaultMaxRows = 5_000;

    /// <summary>The most a dump ever reads, whatever budget it is handed.</summary>
    public static long MaxBytes(int maxRows = DefaultMaxRows) => (long)maxRows * BytesPerRow;

    public static HexPreview Read(Stream stream, long byteBudget, int maxRows = DefaultMaxRows)
    {
        if (maxRows <= 0) return new HexPreview([], 0, Truncated: false);

        var budget = (int)Math.Clamp(Math.Min(byteBudget, MaxBytes(maxRows)), 0, int.MaxValue - 1);
        var (bytes, moreRemains) = ReadAtMost(stream, budget);

        if (bytes.Length == 0) return new HexPreview([], 0, moreRemains);

        var rows = new List<HexRow>((bytes.Length + BytesPerRow - 1) / BytesPerRow);
        for (var start = 0; start < bytes.Length; start += BytesPerRow)
            rows.Add(Format(bytes, start));

        return new HexPreview(rows, bytes.Length, moreRemains);
    }

    /// <summary>One row. A short final row pads both columns rather than ending early, so the ASCII
    /// gutter stays where the rows above put it instead of sliding left on the last line.</summary>
    private static HexRow Format(byte[] bytes, int start)
    {
        var count = Math.Min(BytesPerRow, bytes.Length - start);
        var row = new StringBuilder(80);

        row.Append(start.ToString("X8"));
        var offsetLength = row.Length;

        row.Append("  ");
        for (var i = 0; i < BytesPerRow; i++)
        {
            // The extra gap between the two halves of eight; without it counting to the eleventh
            // byte of a row means counting to eleven.
            if (i == BytesPerRow / 2) row.Append(' ');
            if (i < count) row.Append(bytes[start + i].ToString("X2"));
            else row.Append("  ");
            row.Append(' ');
        }

        row.Append(' ');
        var asciiStart = row.Length;
        for (var i = 0; i < BytesPerRow; i++)
        {
            if (i >= count) { row.Append(' '); continue; }
            var b = bytes[start + i];
            row.Append(b is >= 0x20 and <= 0x7E ? (char)b : '.');
        }

        // The bytes are the content, so they take the pane's own foreground (SyntaxClass.Text maps
        // to no brush at all); the offset is a gutter and takes the dimmest colour there is; the
        // ASCII column takes the string colour, which is the one furthest from both in every
        // palette. Comment and Number were the obvious pair and are the wrong one — in Dark+ they
        // are two greens, and the offsets disappeared into the bytes beside them.
        var text = row.ToString();
        IReadOnlyList<SyntaxSpan> spans =
        [
            new SyntaxSpan(0, offsetLength, SyntaxClass.Comment),
            new SyntaxSpan(offsetLength, asciiStart - offsetLength, SyntaxClass.Text),
            new SyntaxSpan(asciiStart, text.Length - asciiStart, SyntaxClass.String),
        ];

        return new HexRow(start, text, spans);
    }

    private static (byte[] Bytes, bool MoreRemains) ReadAtMost(Stream stream, int budget)
    {
        // One byte past the budget, purely to answer "is there more?" without seeking — the same
        // trick, and for the same reason, as TextPreviewReader's own bounded read.
        var buffer = new byte[budget + 1];
        var total = 0;
        while (total < buffer.Length)
        {
            var read = stream.Read(buffer, total, buffer.Length - total);
            if (read == 0) break;
            total += read;
        }

        var moreRemains = total > budget;
        var length = Math.Min(total, budget);
        return (buffer[..length], moreRemains);
    }
}
