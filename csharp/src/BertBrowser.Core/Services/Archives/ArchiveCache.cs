using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// The last few archives read, so walking around inside one does not re-read its directory on every
/// step.
/// </summary>
/// <remarks>
/// <para>
/// <b>Keyed by canonical path plus length plus last-write time</b>, which is
/// <c>PreviewImageCache</c>'s discipline and matters more here: a preview that outlived an edit is
/// a stale picture, whereas a <em>listing</em> that outlived an edit is something the user then
/// extracts from. A rewritten archive gets a new key and is read again.
/// </para>
/// <para>
/// Bounded two ways because either alone fails: eight entries would hold a gigabyte if each were a
/// 200,000-entry index, and a byte bound alone would hold thousands of tiny ones. Eviction is
/// least-recently-used, tracked by a counter rather than a clock so the order cannot depend on how
/// fast the machine is.
/// </para>
/// <para>
/// A password is part of the key only in as much as an index read <em>with</em> one supersedes one
/// read without: unlocking re-reads and replaces. The password itself is never stored here — see
/// <c>ArchivePasswordStore</c>.
/// </para>
/// </remarks>
public sealed class ArchiveCache
{
    private readonly record struct Key(string Path, long Length, DateTime WriteUtc);

    private sealed class Slot
    {
        public required ArchiveIndex Index { get; init; }
        public required long Weight { get; init; }
        public required bool Unlocked { get; init; }
        public long Used { get; set; }
    }

    private readonly int _maxEntries;
    private readonly long _maxWeight;
    private readonly Dictionary<Key, Slot> _slots = [];
    private readonly Lock _gate = new();
    private long _clock;
    private long _weight;

    public ArchiveCache(int maxEntries = 8, long maxWeight = 64L * 1024 * 1024)
    {
        _maxEntries = maxEntries;
        _maxWeight = maxWeight;
    }

    /// <summary>
    /// The index for <paramref name="archiveFile"/>, reading it through <paramref name="read"/>
    /// only when nothing usable is cached. Returns a failure index rather than throwing when the
    /// file cannot even be stat-ed.
    /// </summary>
    public ArchiveIndex Get(
        string archiveFile, string? password, Func<string, string?, ArchiveIndex> read)
    {
        Key key;
        try
        {
            var info = new FileInfo(archiveFile);
            if (!info.Exists)
                return ArchiveIndex.Failed(ArchiveFailure.Unreadable, "The archive is no longer there.");
            key = new Key(PathKey.Canonicalize(archiveFile), info.Length, info.LastWriteTimeUtc);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                       or ArgumentException or NotSupportedException)
        {
            return ArchiveIndex.Failed(ArchiveFailure.Unreadable, "The archive could not be opened.");
        }

        var unlocked = password is not null;

        lock (_gate)
        {
            // A cached index read without a password is not good enough once one has been given:
            // that is exactly the re-read unlocking is asking for.
            if (_slots.TryGetValue(key, out var hit) && (hit.Unlocked || !unlocked))
            {
                hit.Used = ++_clock;
                return hit.Index;
            }
        }

        var index = read(archiveFile, password);

        // A failure is not cached. Re-reading a broken archive costs one open, and caching the
        // failure would make a transient one — a file still being written — stick until it changed.
        if (!index.Ok) return index;

        lock (_gate)
        {
            _slots.Remove(key, out var replaced);
            if (replaced is not null) _weight -= replaced.Weight;

            var weight = Weigh(index);
            _slots[key] = new Slot { Index = index, Weight = weight, Unlocked = unlocked, Used = ++_clock };
            _weight += weight;
            Evict();
        }
        return index;
    }

    /// <summary>Forgets everything. Used when the app can no longer vouch for what it holds.</summary>
    public void Clear()
    {
        lock (_gate)
        {
            _slots.Clear();
            _weight = 0;
        }
    }

    private void Evict()
    {
        while (_slots.Count > _maxEntries || (_weight > _maxWeight && _slots.Count > 1))
        {
            var oldest = default(Key);
            var oldestUsed = long.MaxValue;
            foreach (var (key, slot) in _slots)
            {
                if (slot.Used >= oldestUsed) continue;
                oldest = key;
                oldestUsed = slot.Used;
            }
            if (!_slots.Remove(oldest, out var gone)) break;
            _weight -= gone.Weight;
        }
    }

    /// <summary>
    /// A rough byte cost. Node count times a per-node estimate — the point is to be proportional,
    /// not exact; an exact figure would cost more to compute than it saves.
    /// </summary>
    private static long Weigh(ArchiveIndex index) => (long)index.ByPath.Count * 256;
}
