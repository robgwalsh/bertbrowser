using BertBrowser.Core.Services.Search;

namespace BertBrowser.Core.Services.Commands;

/// <summary>One row of a listing, as "select by pattern" needs to see it.</summary>
public readonly record struct SelectionRow(
    string Name,
    string FullPath,
    bool IsDirectory,
    long SizeBytes,
    DateTime ModifiedUtc,
    DateTime CreatedUtc,
    FileAttributes Attributes);

/// <summary>Which rows a pattern picks, or why it could not be used.</summary>
/// <param name="Indexes">The matching rows, by position in the list that was asked about.</param>
public sealed record SelectionMatch(IReadOnlyList<int> Indexes, string? Problem);

/// <summary>
/// "Select by pattern": which rows of the listing on screen a typed pattern picks.
/// </summary>
/// <remarks>
/// The pattern is the search box's own language, parsed by the same grammar — so <c>*.jpg</c>,
/// <c>ext:png size:&gt;1mb</c> and <c>re:^IMG_\d+</c> all mean here exactly what they mean there,
/// and there is one syntax to learn rather than a second, smaller one for selecting. What differs
/// is the subject: this judges the rows already listed and reads nothing, so a term that needs a
/// file opened (<c>content:</c>) or names another place to look (<c>in:archives</c>) is refused
/// rather than answered wrongly.
/// </remarks>
public static class SelectionPattern
{
    public static SelectionMatch Match(string? pattern, IReadOnlyList<SelectionRow> rows)
    {
        // No floor on how broad the pattern may be: it is judged against the rows on screen, not
        // run over a disk, so "is:readonly" or "*" alone is a fair thing to ask.
        var parse = SearchGrammar.Parse(pattern, requireNarrow: false);
        if (parse.Problem is { } problem) return new SelectionMatch([], problem);
        if (parse.Query is not { } query) return new SelectionMatch([], null);

        if (query.NeedsContent)
            return new SelectionMatch([], "content: reads files, which selecting does not do. Search for it instead.");
        if (query.WantsArchives)
            return new SelectionMatch([], "in:archives looks inside containers, which selecting does not do.");

        var matched = new List<int>();
        for (var i = 0; i < rows.Count; i++)
        {
            var row = rows[i];
            var candidate = new SearchCandidate(
                row.Name.ToUpperInvariant(),
                row.FullPath.ToUpperInvariant(),
                row.IsDirectory,
                row.SizeBytes,
                row.ModifiedUtc,
                // A hidden row is on screen because hidden items are being shown, so it is as
                // selectable as its neighbours; is:hidden still tells them apart by the mask.
                Hidden: row.Attributes.HasFlag(FileAttributes.Hidden),
                row.Attributes,
                row.CreatedUtc);

            if (query.Evaluate(candidate) == SearchMatch.Yes) matched.Add(i);
        }

        return new SelectionMatch(matched, null);
    }
}
