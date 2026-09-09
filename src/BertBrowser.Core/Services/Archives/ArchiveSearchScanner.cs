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
    /// <param name="query">
    /// Null lists every entry instead of matching one, which is what the flat branch view asks for
    /// inside an open container. It costs nothing here: this walk was always the whole subtree with
    /// a filter over it, and the container's index is already in memory.
    /// </param>
    /// <param name="includeDirectories">False lists files only, as the flat view's default does.
    /// The walk still descends — what is emitted and what is recursed into are separate.</param>
    public static IReadOnlyList<SearchHit> Search(
        ArchiveIndex index,
        string archiveFile,
        string relativeTo,
        SearchQuery? query,
        int limit,
        CancellationToken ct = default,
        bool includeDirectories = true)
    {
        var hits = new List<SearchHit>();
        if (!index.Ok) return hits;

        // A content query has no answer in here, and the failure mode is the dangerous kind rather
        // than the obvious one. An entry has no file on disk to open, so its candidate carries no
        // content — and an unread candidate counts as a *possible* match by design, which is what
        // lets the first pass shortlist. Run the walk anyway and every entry in the container comes
        // back as a hit. The grammar refuses `content: in:archives` before it gets here; this is
        // the guard that does not depend on two callers remembering.
        if (query?.NeedsContent == true) return hits;

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

            if (!includeDirectories && node.IsDirectory) continue;

            var virtualPath = ArchivePath.Compose(archiveFile, node.Path);

            if (query is null)
            {
                hits.Add(Hit(virtualPath, node, relativeTo));
                continue;
            }

            var candidate = new SearchCandidate(
                node.Name.ToUpperInvariant(),
                virtualPath.ToUpperInvariant(),
                node.IsDirectory,
                node.SizeBytes,
                node.Modified?.ToUniversalTime() ?? DateTime.MinValue,
                // Nothing inside an archive is hidden — the listing takes the same view, and for
                // the same reason: the attribute a container carries means different things
                // depending on which tool wrote it. The rest of the mask is absent for that same
                // reason, so is:readonly and friends answer "no" here rather than guessing from a
                // Unix mode or a zip's "external attributes" field. A container directory records
                // one timestamp, and it is the modified one, so there is no creation date either.
                Hidden: false,
                Attributes: 0,
                CreatedUtc: default);

            if (!query.Matches(candidate)) continue;

            hits.Add(Hit(virtualPath, node, relativeTo));
        }

        return hits;
    }

    private static SearchHit Hit(string virtualPath, ArchiveNode node, string relativeTo) =>
        new(virtualPath,
            RelativeDirDisplay(node.Path, relativeTo),
            node.Name,
            node.IsDirectory,
            node.SizeBytes,
            node.Modified?.ToUniversalTime() ?? default);

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
