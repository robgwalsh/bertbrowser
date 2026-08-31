using System.Text;

namespace BertBrowser.Core.Services.Preview;

/// <summary>What a text preview turned out to be. Everything the pane's footer says comes from
/// here, so nothing about the read is inferred twice.</summary>
/// <param name="Text">The decoded text, with line endings normalised to <c>\n</c>.</param>
/// <param name="EncodingName">How it was decoded, in words a person would use.</param>
/// <param name="LineEnding">"CRLF", "LF", "CR", "Mixed", or empty for a single-line file.</param>
/// <param name="LineCount">Lines in <paramref name="Text"/>, not in the file — they differ when
/// <paramref name="Truncated"/> is set.</param>
/// <param name="Truncated">The file has more than this. Said out loud rather than hidden: a
/// preview that silently stops is worse than no preview, because it looks like the whole file.</param>
/// <param name="LooksBinary">Not text at all. <paramref name="Text"/> is empty.</param>
public sealed record TextPreview(
    string Text,
    string EncodingName,
    string LineEnding,
    int LineCount,
    bool Truncated,
    bool LooksBinary);

/// <summary>
/// Decodes the front of a file for display. Pure over a <see cref="Stream"/> — it never opens
/// anything, which is what lets every rule in it be tested against a <see cref="MemoryStream"/>.
/// </summary>
/// <remarks>
/// There is no encoding detection in .NET and no dependency here to borrow one from, so this is
/// the usual ladder: byte-order mark, then a strict UTF-8 validation, then a UTF-16-without-a-BOM
/// heuristic, then Latin-1 as the fallback that cannot fail. Latin-1 rather than the machine's ANSI
/// codepage on purpose — it maps every byte to a character, never throws, never substitutes, and
/// gives the same answer on every machine, which is what makes the tests mean anything.
/// </remarks>
public static class TextPreviewReader
{
    /// <summary>Lines beyond this are not shown. A minified bundle is one line and a log is
    /// millions; both need a ceiling, and the line ceiling is the one a text control cares about.</summary>
    public const int DefaultMaxLines = 5_000;

    /// <summary>Characters beyond this are folded onto the next line. The line <em>count</em> cap
    /// above does not bound the work a text control does, because one line can be the whole budget:
    /// a minified bundle is one, and a binary read under <c>forceText</c> almost always is. A single
    /// megabyte-long run in a non-wrapping <c>RichTextBox</c> is a stall, so the length is capped
    /// here, beside the count, rather than in the view where no test could reach it. At 4,096 it is
    /// inert for anything a person wrote.</summary>
    public const int DefaultMaxLineLength = 4_096;

    /// <summary>
    /// Whether a read is convincing enough to show as text when nothing <em>said</em> it was text.
    /// </summary>
    /// <remarks>
    /// The stricter half of the pair. <see cref="TextPreview.LooksBinary"/> answers "did this file
    /// with a text extension turn out to be binary?", which only has to catch the obvious case; this
    /// answers "we are guessing — is this worth showing?", and a wrong yes puts a screen of mojibake
    /// where an honest "no preview available" belonged.
    ///
    /// The extra test is the proportion of control characters. A binary file with no NUL in its
    /// first 8 KB gets past the NUL check, but decoding it as Latin-1 turns bytes 0x00–0x1F and
    /// 0x80–0x9F into control characters, and real text has almost none of those — tab, newline and
    /// carriage return being the three that are ordinary and so are not counted.
    /// </remarks>
    public static bool IsConvincingText(TextPreview preview)
    {
        if (preview.LooksBinary) return false;

        // An empty file is empty text, not a mystery — showing it as a blank text pane is a better
        // answer than refusing it.
        if (preview.Text.Length == 0) return true;

        var sample = Math.Min(preview.Text.Length, 4096);
        var control = 0;
        for (var i = 0; i < sample; i++)
        {
            var c = preview.Text[i];
            if (char.IsControl(c) && c is not ('\t' or '\n' or '\r')) control++;
        }

        return control * 100 < sample * 2; // under 2%
    }

    /// <param name="forceText">Decode the bytes even when they are plainly not text. This is raw
    /// mode: it skips the NUL rung of the ladder and <em>only</em> that rung, so a forced read of a
    /// UTF-16 file is still UTF-16 and everything else lands on Latin-1, which maps every byte.
    /// It is what lets the pane show a PDF's header.</param>
    public static TextPreview Read(
        Stream stream,
        long byteBudget,
        int maxLines = DefaultMaxLines,
        int maxLineLength = DefaultMaxLineLength,
        bool forceText = false)
    {
        var budget = (int)Math.Clamp(byteBudget, 0, int.MaxValue - 1);
        var (bytes, moreRemains) = ReadAtMost(stream, budget);

        if (bytes.Length == 0)
            return new TextPreview("", "UTF-8", "", 0, moreRemains, LooksBinary: false);

        var (encoding, name, bomLength) = DetectEncoding(bytes, moreRemains, forceText);
        if (encoding is null)
            return new TextPreview("", "Binary", "", 0, moreRemains, LooksBinary: true);

        ReadOnlySpan<byte> body = bytes.AsSpan(bomLength);

        // A budget cut lands wherever it lands, which for a multi-byte encoding is often mid
        // character. Trimming the partial tail is cheaper than explaining the replacement glyph
        // that would otherwise appear at the end of every truncated preview.
        if (moreRemains)
            body = TrimPartialTail(body, encoding);

        var text = encoding.GetString(body);
        var lineEnding = DetectLineEnding(text);
        text = Normalise(text);
        if (forceText) text = Dot(text);
        text = FoldLongLines(text, maxLineLength);

        var truncated = moreRemains;
        var lineCount = CountLines(text);
        if (lineCount > maxLines)
        {
            text = TakeLines(text, maxLines);
            lineCount = maxLines;
            truncated = true;
        }

        return new TextPreview(text, name, lineEnding, lineCount, truncated, LooksBinary: false);
    }

    private static (byte[] Bytes, bool MoreRemains) ReadAtMost(Stream stream, int budget)
    {
        // One byte past the budget, purely to answer "is there more?" without seeking or asking
        // the caller for a length it may not have.
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

    /// <summary>Null encoding means the bytes are not text.</summary>
    private static (Encoding? Encoding, string Name, int BomLength) DetectEncoding(
        byte[] bytes, bool truncated, bool forceText = false)
    {
        if (StartsWith(bytes, 0xEF, 0xBB, 0xBF)) return (new UTF8Encoding(false), "UTF-8 (BOM)", 3);
        if (StartsWith(bytes, 0xFF, 0xFE, 0x00, 0x00)) return (new UTF32Encoding(false, false), "UTF-32 LE", 4);
        if (StartsWith(bytes, 0x00, 0x00, 0xFE, 0xFF)) return (new UTF32Encoding(true, false), "UTF-32 BE", 4);
        if (StartsWith(bytes, 0xFF, 0xFE)) return (new UnicodeEncoding(false, false), "UTF-16 LE", 2);
        if (StartsWith(bytes, 0xFE, 0xFF)) return (new UnicodeEncoding(true, false), "UTF-16 BE", 2);

        var bomless = DetectBomlessUtf16(bytes);
        if (bomless is not null) return bomless.Value;

        // Raw mode skips this rung and only this one. The BOM checks and the bomless-UTF-16
        // heuristic above still ran, so a forced read of a UTF-16 file is still UTF-16; what is
        // given up is the right to say "binary", which is exactly what raw mode was asked for.
        if (!forceText && HasNul(bytes)) return (null, "Binary", 0);

        return IsValidUtf8(bytes, truncated)
            ? (new UTF8Encoding(false), "UTF-8", 0)
            : (Encoding.Latin1, "Latin-1", 0);
    }

    /// <summary>UTF-16 with no BOM is what most Windows tools emit, and it is the one case where
    /// NUL bytes mean text rather than binary. The tell is that they are all on one parity.</summary>
    private static (Encoding, string, int)? DetectBomlessUtf16(byte[] bytes)
    {
        var sample = Math.Min(bytes.Length, 8192);
        if (sample < 4) return null;

        int evenNuls = 0, oddNuls = 0;
        for (var i = 0; i < sample; i++)
        {
            if (bytes[i] != 0) continue;
            if (i % 2 == 0) evenNuls++; else oddNuls++;
        }

        var pairs = sample / 2;
        if (pairs == 0) return null;

        // Three quarters of the high bytes zero, and essentially none of the low bytes: that is
        // ASCII-range text in a two-byte encoding, not a binary file that happens to have zeros.
        if (oddNuls * 4 >= pairs * 3 && evenNuls * 20 < pairs)
            return (new UnicodeEncoding(false, false), "UTF-16 LE", 0);
        if (evenNuls * 4 >= pairs * 3 && oddNuls * 20 < pairs)
            return (new UnicodeEncoding(true, false), "UTF-16 BE", 0);
        return null;
    }

    private static bool StartsWith(byte[] bytes, params byte[] prefix)
    {
        if (bytes.Length < prefix.Length) return false;
        for (var i = 0; i < prefix.Length; i++)
            if (bytes[i] != prefix[i]) return false;
        return true;
    }

    private static bool HasNul(byte[] bytes)
    {
        var sample = Math.Min(bytes.Length, 8192);
        for (var i = 0; i < sample; i++)
            if (bytes[i] == 0) return true;
        return false;
    }

    /// <param name="truncated">Whether the budget cut the read short. A sequence running off the
    /// end is evidence of the cut when it did, and evidence against UTF-8 when it did not — which
    /// is the difference between reading a Latin-1 file correctly and mangling it.</param>
    private static bool IsValidUtf8(byte[] bytes, bool truncated)
    {
        var i = 0;
        while (i < bytes.Length)
        {
            var b = bytes[i];
            int continuations;
            if (b < 0x80) { i++; continue; }
            else if (b >= 0xC2 && b <= 0xDF) continuations = 1;
            else if (b >= 0xE0 && b <= 0xEF) continuations = 2;
            else if (b >= 0xF0 && b <= 0xF4) continuations = 3;
            else return false;

            if (i + continuations >= bytes.Length) return truncated;

            for (var k = 1; k <= continuations; k++)
                if ((bytes[i + k] & 0xC0) != 0x80) return false;
            i += continuations + 1;
        }
        return true;
    }

    private static ReadOnlySpan<byte> TrimPartialTail(ReadOnlySpan<byte> body, Encoding encoding)
    {
        if (encoding is UTF32Encoding) return body[..(body.Length - body.Length % 4)];
        if (encoding is UnicodeEncoding) return body[..(body.Length - body.Length % 2)];
        if (encoding is not UTF8Encoding) return body;

        // Walk back over continuation bytes to the lead byte, and drop the sequence if the bytes
        // it needs are not all here.
        var end = body.Length;
        var back = 0;
        while (end - 1 - back >= 0 && (body[end - 1 - back] & 0xC0) == 0x80 && back < 3) back++;
        var leadIndex = end - 1 - back;
        if (leadIndex < 0) return body;

        var lead = body[leadIndex];
        var needed = lead switch
        {
            >= 0xF0 => 4,
            >= 0xE0 => 3,
            >= 0xC0 => 2,
            _ => 1,
        };
        return needed > 1 && end - leadIndex < needed ? body[..leadIndex] : body;
    }

    private static string DetectLineEnding(string text)
    {
        bool crlf = false, lf = false, cr = false;
        for (var i = 0; i < text.Length; i++)
        {
            if (text[i] == '\r')
            {
                if (i + 1 < text.Length && text[i + 1] == '\n') { crlf = true; i++; }
                else cr = true;
            }
            else if (text[i] == '\n') lf = true;
        }
        var kinds = (crlf ? 1 : 0) + (lf ? 1 : 0) + (cr ? 1 : 0);
        if (kinds == 0) return "";
        if (kinds > 1) return "Mixed";
        return crlf ? "CRLF" : lf ? "LF" : "CR";
    }

    /// <summary>Every line ending becomes <c>\n</c>, so offsets into the text mean one thing —
    /// the tokenizer's spans and the view's line numbers both index this string.</summary>
    private static string Normalise(string text)
    {
        if (!text.Contains('\r')) return text;
        return text.Replace("\r\n", "\n").Replace('\r', '\n');
    }

    /// <summary>
    /// Raw mode only: every control character but tab and newline becomes a dot, the way the hex
    /// dump's ASCII column already spells them.
    /// </summary>
    /// <remarks>
    /// This is not cosmetic. WPF's text layout treats U+000B, U+000C, U+0085, U+2028 and U+2029 as
    /// line breaks in their own right, and Latin-1 turns byte 0x85 into the third of those — so a
    /// binary read of any file containing one renders more rows than the string has lines, and the
    /// gutter beside it stops lining up from that point down. It is also the honest rendering: in a
    /// file being read as bytes, 0x85 is a byte, not a paragraph mark.
    ///
    /// Ordinary text is deliberately left alone. There those characters really are separators, and
    /// they are rare enough that dotting them would cost more than it saved.
    /// </remarks>
    private static string Dot(string text)
    {
        var needed = false;
        foreach (var c in text)
        {
            if (!IsUnshowable(c)) continue;
            needed = true;
            break;
        }
        if (!needed) return text;

        var clean = new char[text.Length];
        for (var i = 0; i < text.Length; i++)
            clean[i] = IsUnshowable(text[i]) ? '.' : text[i];
        return new string(clean);
    }

    private static bool IsUnshowable(char c) =>
        // char.IsControl covers C0 and C1 — which is where 0x85 lives — but not the two Unicode
        // separators, and WPF breaks a line on those just as readily. Written as escapes because a
        // literal U+2028 in a source file is a line terminator to the C# compiler too.
        (char.IsControl(c) && c is not ('\t' or '\n')) || c is '\u2028' or '\u2029';

    /// <summary>Breaks any line longer than <paramref name="maxLength"/> into pieces of that
    /// length. Runs after <see cref="Normalise"/> so there is one line terminator to look for, and
    /// before the line count, so the count describes what will actually be shown.</summary>
    private static string FoldLongLines(string text, int maxLength)
    {
        if (maxLength <= 0) return text;

        // Almost every file is under the cap on every line, and walking once to find that out
        // costs nothing next to building a second string that would be identical.
        var longest = 0;
        var start = 0;
        for (var i = 0; i <= text.Length; i++)
        {
            if (i != text.Length && text[i] != '\n') continue;
            longest = Math.Max(longest, i - start);
            start = i + 1;
        }
        if (longest <= maxLength) return text;

        var folded = new StringBuilder(text.Length + text.Length / maxLength + 1);
        var run = 0;
        foreach (var c in text)
        {
            if (c == '\n')
            {
                folded.Append(c);
                run = 0;
                continue;
            }
            if (run == maxLength)
            {
                folded.Append('\n');
                run = 0;
            }
            folded.Append(c);
            run++;
        }
        return folded.ToString();
    }

    private static int CountLines(string text)
    {
        if (text.Length == 0) return 0;
        var lines = 1;
        foreach (var c in text)
            if (c == '\n') lines++;
        // A file ending in a newline has no empty last line worth counting.
        return text[^1] == '\n' ? lines - 1 : lines;
    }

    private static string TakeLines(string text, int maxLines)
    {
        var seen = 0;
        for (var i = 0; i < text.Length; i++)
        {
            if (text[i] != '\n') continue;
            if (++seen == maxLines) return text[..i];
        }
        return text;
    }
}
