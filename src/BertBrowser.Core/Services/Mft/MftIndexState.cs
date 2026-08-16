using System.Collections.Concurrent;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// What an <see cref="IMftIndexService"/> knows: which volumes are still building and which have
/// finished, and the one status line that follows from it.
/// </summary>
/// <remarks>
/// <para>
/// This is separate from <see cref="MftIndexService"/> because there are now two implementations of
/// that interface — the real indexer, and <c>MftIndexClient</c>, which mirrors an indexer running
/// in another process. Both have to answer <see cref="IMftIndexService.IsIndexed"/> and produce
/// <see cref="IMftIndexService.StatusText"/> identically, or search routing and the status bar
/// disagree depending on which one is wired up. One implementation, and it is unit-testable
/// without a volume to index.
/// </para>
/// <para>
/// Building is keyed by drive letter (what the caller is working through) and completion by root
/// key (what callers ask about), which is why they are separate maps rather than one.
/// </para>
/// </remarks>
public sealed class MftIndexState
{
    private readonly ConcurrentDictionary<string, byte> _completedRoots = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, byte> _building = new(StringComparer.Ordinal);

    /// <summary>True once at least one volume's initial enumeration has completed.</summary>
    public bool AnyIndexed => !_completedRoots.IsEmpty;

    /// <summary>True while any volume's initial enumeration is still running.</summary>
    public bool IsBuilding => !_building.IsEmpty;

    /// <summary>
    /// True if <paramref name="pathKey"/> sits on a volume whose index is complete.
    /// </summary>
    /// <remarks>
    /// Goes through <see cref="PathKey.IsUnder"/> rather than <c>StartsWith</c>: a root of
    /// <c>C:\FOO</c> must not claim <c>C:\FOOBAR</c>, and only the half-open prefix bounds get that
    /// right.
    /// </remarks>
    public bool IsIndexed(string pathKey)
    {
        foreach (var root in _completedRoots.Keys)
        {
            if (pathKey.Equals(root, StringComparison.Ordinal) || PathKey.IsUnder(pathKey, root))
                return true;
        }
        return false;
    }

    /// <summary>Every root that has finished building, for a client re-sending its state.</summary>
    public IReadOnlyCollection<string> CompletedRoots => _completedRoots.Keys.ToArray();

    /// <summary>The bare drive letters currently building, for a host relaying its state.</summary>
    public IReadOnlyCollection<string> BuildingDrives => _building.Keys.ToArray();

    /// <summary>Marks <paramref name="drive"/> (a bare letter) as building.</summary>
    public void MarkBuilding(string drive) => _building[drive] = 0;

    /// <summary>Marks <paramref name="drive"/> as no longer building, whether it finished or failed.</summary>
    public void ClearBuilding(string drive) => _building.TryRemove(drive, out _);

    /// <summary>Marks <paramref name="rootKey"/> as fully indexed.</summary>
    public void MarkComplete(string rootKey) => _completedRoots[rootKey] = 0;

    /// <summary>Forgets everything — used when a client loses its indexer.</summary>
    public void Clear()
    {
        _completedRoots.Clear();
        _building.Clear();
    }

    /// <summary>The status-bar line for the current building set; empty when nothing is building.</summary>
    public string FormatStatus()
    {
        var building = _building.Keys.OrderBy(d => d, StringComparer.Ordinal).ToList();
        return building.Count switch
        {
            0 => "",
            1 => $"Indexing {building[0]}:…",
            _ => $"Indexing {string.Join(", ", building.Select(d => d + ":"))}…",
        };
    }
}
