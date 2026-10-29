using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Bench.Fakes;

/// <summary>
/// An in-memory disk the planners can probe: a set of files and folders, nothing more.
/// </summary>
/// <remarks>
/// The planners decide through probe interfaces precisely so they can be tested without a disk, and
/// the same seam is what lets them be measured without one — what is timed is the planning, not
/// <c>File.Exists</c>. Lifted from the private fakes in <c>BertBrowser.Core.Tests</c> rather than
/// shared with them; a hundred lines twice beats widening the test project's surface.
/// </remarks>
internal sealed class FakeDisk : ITransferEntrySource, IRenameProbe, IDeleteProbe, IRecycleProbe
{
    private static readonly DateTime Stamp = new(2026, 9, 15, 12, 0, 0, DateTimeKind.Utc);

    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly Dictionary<string, long> _files = new(StringComparer.Ordinal);
    private readonly Dictionary<string, List<TransferEntry>> _children = new(StringComparer.Ordinal);

    public FakeDisk AddDirectory(string path)
    {
        var key = PathKey.Canonicalize(path);
        if (_directories.Add(key))
            Parent(key).Add(new TransferEntry(Path.GetFileName(path), path, true, false, 0, Stamp));
        return this;
    }

    public FakeDisk AddFile(string path, long size = 1024)
    {
        var key = PathKey.Canonicalize(path);
        if (_files.TryAdd(key, size))
            Parent(key).Add(new TransferEntry(Path.GetFileName(path), path, false, false, size, Stamp));
        return this;
    }

    private List<TransferEntry> Parent(string key)
    {
        var parent = Path.GetDirectoryName(key) ?? key;
        if (!_children.TryGetValue(parent, out var list))
            _children[parent] = list = [];
        return list;
    }

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.ContainsKey(PathKey.Canonicalize(path));

    public string ResolveFinalPath(string path) => path;

    public IReadOnlyList<TransferEntry> Entries(string directory) =>
        _children.TryGetValue(PathKey.Canonicalize(directory), out var list) ? list : [];

    public bool CanRecycle(string path) => true;
}
