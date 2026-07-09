using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// Rolls per-file sizes up the directory tree to produce a <see cref="DirSizeResult"/> for every
/// directory on the volume — the instant-folder-size trick. Fed the same <see cref="MftReader"/>
/// stream that builds the search index, it buckets each file onto its direct parent, then does one
/// iterative post-order pass from the root record (5) summing subtree bytes / file / dir counts.
/// Reparse-point junctions have no MFT children under their own record, so they contribute nothing
/// — matching the DFS scanner's deliberate skip. Because the raw read sees every record regardless
/// of ACLs, results are never <c>incomplete</c>.
/// </summary>
internal sealed class MftDirectorySizeBuilder
{
    private readonly Dictionary<ulong, MftNode> _dirNodes = new();
    private readonly Dictionary<ulong, long> _directBytes = new();
    private readonly Dictionary<ulong, int> _directFiles = new();
    private readonly Dictionary<ulong, List<ulong>> _childDirs = new();

    public void Add(in MftFileRecord rec)
    {
        if (rec.IsDirectory)
        {
            _dirNodes[rec.RecordNumber] = new MftNode(rec.Name, rec.ParentRecordNumber, true, rec.Hidden);
            if (!_childDirs.TryGetValue(rec.ParentRecordNumber, out var kids))
                _childDirs[rec.ParentRecordNumber] = kids = new List<ulong>();
            kids.Add(rec.RecordNumber);
        }
        else
        {
            _directBytes[rec.ParentRecordNumber] = _directBytes.GetValueOrDefault(rec.ParentRecordNumber) + rec.Size;
            _directFiles[rec.ParentRecordNumber] = _directFiles.GetValueOrDefault(rec.ParentRecordNumber) + 1;
        }
    }

    /// <summary>
    /// Computes every directory's subtree totals and returns one <see cref="DirSizeResult"/> per
    /// directory. <paramref name="dirCache"/> is filled with each directory's resolved display
    /// path (and effective-hidden state) as a side effect — the caller reuses it both to build the
    /// file rows of the search index and to seed the USN tail's resident directory map.
    /// </summary>
    public List<DirSizeResult> Build(
        string driveRoot, DateTime computedUtc, Dictionary<ulong, (string Path, bool Hidden)> dirCache)
    {
        var subtreeBytes = new Dictionary<ulong, long>(_dirNodes.Count);
        var subtreeFiles = new Dictionary<ulong, int>(_dirNodes.Count);
        var subtreeDirs = new Dictionary<ulong, int>(_dirNodes.Count);
        var results = new List<DirSizeResult>(_dirNodes.Count);

        var seen = new HashSet<ulong>();
        var done = new HashSet<ulong>();
        var stack = new Stack<(ulong Frn, bool Expanded)>();
        foreach (var top in _childDirs.GetValueOrDefault(NtfsLayout.RootRecordNumber, new List<ulong>()))
            if (seen.Add(top)) stack.Push((top, false));

        while (stack.Count > 0)
        {
            var (frn, expanded) = stack.Pop();
            if (!expanded)
            {
                stack.Push((frn, true));
                foreach (var child in _childDirs.GetValueOrDefault(frn, new List<ulong>()))
                    if (seen.Add(child)) stack.Push((child, false));
                continue;
            }
            if (!done.Add(frn))
                continue;

            var bytes = _directBytes.GetValueOrDefault(frn);
            var files = _directFiles.GetValueOrDefault(frn);
            var dirs = 0;
            foreach (var child in _childDirs.GetValueOrDefault(frn, new List<ulong>()))
            {
                bytes += subtreeBytes.GetValueOrDefault(child);
                files += subtreeFiles.GetValueOrDefault(child);
                dirs += subtreeDirs.GetValueOrDefault(child) + 1;
            }
            subtreeBytes[frn] = bytes;
            subtreeFiles[frn] = files;
            subtreeDirs[frn] = dirs;

            if (MftPathBuilder.TryResolve(_dirNodes, frn, driveRoot, dirCache, out var display, out _, NtfsLayout.RootRecordNumber))
                results.Add(new DirSizeResult(display.ToUpperInvariant(), bytes, files, dirs, Incomplete: false, computedUtc));
        }

        return results;
    }
}
