using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.Search;

/// <summary>
/// Turns "this file matched" into "line 42, and here it is" — pure, so every rule about clipping
/// and counting is testable without a file.
/// </summary>
public static class ContentSnippet
{
    /// <summary>How much of a matching line is worth showing in a column.</summary>
    /// <remarks>
    /// The reader deliberately lifts the preview pane's line-length fold, because folding would
    /// insert breaks that the line numbers here then counted. That leaves the long-line problem to
    /// land somewhere, and this is where: a minified bundle is one line a megabyte long, and
    /// putting it in a list cell would stall the whole window.
    /// </remarks>
    public const int MaxLineChars = 300;

    /// <summary>How much of the line before the match is kept when clipping.</summary>
    /// <remarks>Enough to see what the match is part of, little enough that the needle is still
    /// near the left edge where the eye lands.</remarks>
    public const int LeadingContext = 32;

    private const string Ellipsis = "…";

    /// <summary>
    /// The first place any needle appears, or null when none does.
    /// </summary>
    /// <remarks>
    /// <strong>First-occurring wins, not first-listed.</strong> With <c>content:beta
    /// content:alpha</c> over a file where alpha comes first, showing beta's line would point at
    /// the wrong part of the file for no reason a reader could work out.
    /// </remarks>
    public static ContentMatch? For(ContentText content, IReadOnlyList<string> needles)
    {
        ArgumentNullException.ThrowIfNull(content);
        ArgumentNullException.ThrowIfNull(needles);

        var text = content.Text;
        var at = -1;
        var length = 0;

        for (var i = 0; i < needles.Count; i++)
        {
            var found = content.IndexOf(needles[i]);
            if (found < 0) continue;
            if (at >= 0 && found >= at) continue;
            at = found;
            length = needles[i].Length;
        }

        if (at < 0) return null;

        var lineStart = text.LastIndexOf('\n', Math.Max(at - 1, 0)) + 1;
        if (at == 0) lineStart = 0;

        var lineEnd = text.IndexOf('\n', at);
        if (lineEnd < 0) lineEnd = text.Length;

        // The reader normalises every line ending to \n, so there is one terminator to find — but
        // a lone \r can still survive inside a line of genuinely mixed endings.
        var line = text[lineStart..lineEnd].TrimEnd('\r');

        // 1-based, counted over the text as read. Truncation is the outcome's business to report,
        // not something to encode in a line number.
        var lineNumber = CountNewlines(text, lineStart) + 1;

        var offset = at - lineStart;
        var (clipped, clippedOffset) = Clip(line, offset, length);

        return new ContentMatch(lineNumber, clipped, clippedOffset, length);
    }

    private static int CountNewlines(string text, int upTo)
    {
        var count = 0;
        for (var i = 0; i < upTo; i++)
            if (text[i] == '\n') count++;
        return count;
    }

    /// <summary>
    /// Keeps a window around the match when the line is too long to show, and reports where the
    /// match ended up in what is left.
    /// </summary>
    private static (string Line, int Offset) Clip(string line, int offset, int length)
    {
        if (line.Length <= MaxLineChars) return (line, offset);

        var start = Math.Max(0, offset - LeadingContext);

        // Never clip so late that the match itself falls off the right-hand end — a snippet whose
        // highlight is out of range is worse than a longer one.
        start = Math.Min(start, Math.Max(0, line.Length - MaxLineChars));
        var end = Math.Min(line.Length, start + MaxLineChars);

        var head = start > 0 ? Ellipsis : "";
        var tail = end < line.Length ? Ellipsis : "";
        var clipped = head + line[start..end] + tail;

        return (clipped, offset - start + head.Length);
    }
}
