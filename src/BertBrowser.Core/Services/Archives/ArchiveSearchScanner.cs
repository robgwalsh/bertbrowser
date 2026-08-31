using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Runs a parsed query over an archive's already-loaded index.
/// </summary>
/// <remarks>
/// <para>
/// <b>It reuses <see cref="SearchNode.Matches"/> verbatim</b> by building a
/// <see cref="SearchCandidate"/> per node, which is the whole reason this is ten lines rather than
/// a second query engine. Everything the box understands works in here for free — <c>ext:</c>,
/// <c>size:</c>, <c>re:</c>, <c>OR</c>, <c>!</c>, brackets — and <c>dm:</c> correctly matches
/// nothing for an entry the container gave no timestamp, because of the 1601 floor those terms
/// already apply.
/// </para>
/// <para>
/// Nothing here touches the index or the disk. Searching inside a container is answered from what
/// the listing already read, which is why it is instant — and is also what keeps the hard
/// invariant: a virtual path must never reach a <c>PathKey</c>-keyed table.
/// </para>
/// </remarks>
public static class ArchiveSearchScanner
{
    /// <summary>
    /// Every entry under <paramref name="relativeTo"/> that the query matches, as search hits with
    /// virtual paths.
    /// </summary>
    /// <param name="root">Where the results are reported relative to — the search root.</param>
    public static IReadOnlyList<SearchHit> Search(
        ArchiveIndex index,
        string archiveFile,
        string relativeTo,
        SearchQuery query,
        int limit,
        CancellationToken ct = default)
    {
        var hits = new List<SearchHit>();
        if (!index.Ok) return hits;

        var start = index.Find(relativeTo);
        if (start is null) return hits;

        var stack = new Stack<ArchiveNode>();
        foreach (var child in start.Children ?? []) stack.Push(child);

        while (stack.Count > 0 && hits.Count < limit)
        {
            ct.ThrowIfCancellationRequested();
            var node = stack.Pop();

            if (node.IsDirectory)
                foreach (var child in node.Children ?? []) stack.Push(child);

            var virtualPath = ArchivePath.Compose(archiveFile, node.Path);

            var candidate = new SearchCandidate(
                node.Name.ToUpperInvariant(),
                virtualPath.ToUpperInvariant(),
                node.IsDirectory,
                node.SizeBytes,
                node.Modified?.ToUniversalTime() ?? DateTime.MinValue,
                // Nothing inside an archive is hidden — the listing takes the same view, and for
                // the same reason: the attribute a container carries means different things
                // depending on which tool wrote it.
                Hidden: false);

            if (!query.Matches(candidate)) continue;

            hits.Add(new SearchHit(
                virtualPath,
                RelativeDirDisplay(node.Path, relativeTo),
                node.Name,
                node.IsDirectory,
                node.SizeBytes,
                node.Modified?.ToUniversalTime() ?? default));
        }

        return hits;
    }

    /// <summary>The folder an entry sits in, relative to the search root — the Folder column.</summary>
    private static string RelativeDirDisplay(string entryPath, string relativeTo)
    {
        var parent = Path.GetDirectoryName(entryPath) ?? "";
        if (relativeTo.Length == 0) return parent;

        var prefix = relativeTo.TrimEnd('\\') + "\\";
        return parent.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? parent[prefix.Length..]
            : parent;
    }
}
