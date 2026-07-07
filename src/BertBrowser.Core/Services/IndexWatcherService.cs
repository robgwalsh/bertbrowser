using System.Collections.Concurrent;
using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

public interface IIndexWatcherService : IDisposable
{
    /// <summary>True if live change events are currently being applied for this indexed root.</summary>
    bool IsWatching(string rootKey);

    /// <summary>
    /// Starts (or refreshes) watching an indexed root. No-op for UNC paths, where
    /// FileSystemWatcher is unreliable — those roots stay on the stale-while-revalidate
    /// path forever.
    /// </summary>
    void Watch(string rootKey, string displayPath);
}

/// <summary>
/// Keeps the search index fresh: one FileSystemWatcher per indexed root patches
/// fs_entry as files are created, changed, deleted, or renamed. Events are queued
/// and drained every 250 ms so bulk operations batch into few transactions. On
/// watcher error/overflow the root is marked stale and the watcher dropped — the
/// next search serves cached results and triggers a re-crawl.
/// </summary>
public sealed class IndexWatcherService : IIndexWatcherService
{
    private const int MaxWatchers = 8;
    private const int MiniCrawlCap = 25_000;
    private static readonly TimeSpan DrainInterval = TimeSpan.FromMilliseconds(250);

    private readonly FsIndexRepository _repository;
    private readonly object _lock = new();
    private readonly Dictionary<string, WatcherEntry> _watchers = new(StringComparer.Ordinal);
    private readonly ConcurrentQueue<FsChange> _pending = new();
    private readonly Timer _drainTimer;
    private int _draining;

    private sealed class WatcherEntry
    {
        public required FileSystemWatcher Watcher;
        public long LastTouched;
    }

    internal sealed record FsChange(string RootKey, WatcherChangeTypes Type, string FullPath, string? OldFullPath);

    public IndexWatcherService(FsIndexRepository repository)
    {
        _repository = repository;
        _drainTimer = new Timer(_ => Drain(), null, DrainInterval, DrainInterval);
    }

    public bool IsWatching(string rootKey)
    {
        lock (_lock)
        {
            if (_watchers.TryGetValue(rootKey, out var entry))
            {
                entry.LastTouched = Environment.TickCount64;
                return true;
            }
            return false;
        }
    }

    public void Watch(string rootKey, string displayPath)
    {
        if (displayPath.StartsWith(@"\\", StringComparison.Ordinal))
            return;

        lock (_lock)
        {
            if (_watchers.TryGetValue(rootKey, out var existing))
            {
                existing.LastTouched = Environment.TickCount64;
                return;
            }

            if (_watchers.Count >= MaxWatchers)
            {
                var lru = _watchers.MinBy(kv => kv.Value.LastTouched);
                lru.Value.Watcher.Dispose();
                _watchers.Remove(lru.Key);
            }

            FileSystemWatcher watcher;
            try
            {
                watcher = new FileSystemWatcher(displayPath)
                {
                    IncludeSubdirectories = true,
                    InternalBufferSize = 64 * 1024,
                    NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName
                                 | NotifyFilters.LastWrite | NotifyFilters.Size,
                };
            }
            catch (Exception ex) when (ex is ArgumentException or FileNotFoundException or IOException)
            {
                return; // root disappeared or is unwatchable; freshness check will fail and re-crawl
            }

            watcher.Created += (_, e) => _pending.Enqueue(new FsChange(rootKey, e.ChangeType, e.FullPath, null));
            watcher.Changed += (_, e) => _pending.Enqueue(new FsChange(rootKey, e.ChangeType, e.FullPath, null));
            watcher.Deleted += (_, e) => _pending.Enqueue(new FsChange(rootKey, e.ChangeType, e.FullPath, null));
            watcher.Renamed += (_, e) => _pending.Enqueue(new FsChange(rootKey, e.ChangeType, e.FullPath, e.OldFullPath));
            watcher.Error += (_, _) => OnWatcherError(rootKey);
            watcher.EnableRaisingEvents = true;

            _watchers[rootKey] = new WatcherEntry { Watcher = watcher, LastTouched = Environment.TickCount64 };
        }
    }

    /// <summary>
    /// Buffer overflow or other watcher failure: events were lost, so the index for
    /// this root can no longer be trusted. Drop the watcher and mark the root stale —
    /// the next search serves cached results instantly and re-crawls in background.
    /// </summary>
    private void OnWatcherError(string rootKey)
    {
        lock (_lock)
        {
            if (_watchers.Remove(rootKey, out var entry))
                entry.Watcher.Dispose();
        }
        try
        {
            _repository.MarkRootStale(rootKey);
        }
        catch (Exception)
        {
            // Best-effort; the root stays "fresh" only until its watcher check fails.
        }
    }

    private void Drain()
    {
        if (Interlocked.CompareExchange(ref _draining, 1, 0) != 0)
            return;
        try
        {
            var changes = new List<FsChange>();
            while (_pending.TryDequeue(out var change))
                changes.Add(change);
            if (changes.Count > 0)
                Apply(changes);
        }
        finally
        {
            Interlocked.Exchange(ref _draining, 0);
        }
    }

    /// <summary>Applies a drained batch of watcher events to the index. Internal for tests.</summary>
    internal void Apply(IReadOnlyList<FsChange> changes)
    {
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        var upserts = new List<FsEntryRow>();

        void FlushUpserts()
        {
            if (upserts.Count == 0) return;
            _repository.UpsertEntries(upserts, crawlGen);
            upserts.Clear();
        }

        foreach (var change in changes)
        {
            try
            {
                switch (change.Type)
                {
                    case WatcherChangeTypes.Created:
                    case WatcherChangeTypes.Changed:
                        CollectUpsert(change, upserts, crawlGen);
                        break;

                    case WatcherChangeTypes.Deleted:
                        FlushUpserts();
                        _repository.DeleteSubtree(PathKey.Canonicalize(change.FullPath));
                        break;

                    case WatcherChangeTypes.Renamed when change.OldFullPath is not null:
                        FlushUpserts();
                        _repository.Rename(
                            PathKey.Canonicalize(change.OldFullPath),
                            PathKey.Canonicalize(change.FullPath),
                            Path.GetFileName(change.FullPath),
                            crawlGen);
                        break;
                }
            }
            catch (Exception)
            {
                // A single unappliable event (transient IO/DB error) must not poison the
                // batch; worst case the entry is wrong until the next re-crawl.
                _repository.MarkRootStale(change.RootKey);
            }
        }
        FlushUpserts();
    }

    private void CollectUpsert(FsChange change, List<FsEntryRow> upserts, long crawlGen)
    {
        var key = PathKey.Canonicalize(change.FullPath);
        var hidden = IsEffectivelyHidden(change.FullPath, change.RootKey);

        if (Directory.Exists(change.FullPath))
        {
            var info = new DirectoryInfo(change.FullPath);
            upserts.Add(new FsEntryRow(key, info.Name, true, 0, info.LastWriteTimeUtc, hidden));

            // A folder moved into the tree raises a single Created event for the top
            // directory only — index its contents with a bounded mini-crawl, seeding the
            // walk with this folder's effective hidden state so descendants inherit it.
            if (change.Type == WatcherChangeTypes.Created)
                MiniCrawl(change, upserts, crawlGen, hidden);
        }
        else if (File.Exists(change.FullPath))
        {
            var info = new FileInfo(change.FullPath);
            upserts.Add(new FsEntryRow(key, info.Name, false, info.Length, info.LastWriteTimeUtc, hidden));
        }
        else
        {
            // Already gone again (temp file churn); treat as a delete.
            _repository.DeleteSubtree(key);
        }
    }

    private void MiniCrawl(FsChange change, List<FsEntryRow> upserts, long crawlGen, bool rootHidden)
    {
        var count = 0;
        var capped = false;
        FileSystemWalker.Walk(change.FullPath, entry =>
        {
            if (++count > MiniCrawlCap)
            {
                capped = true;
                return false;
            }
            upserts.Add(new FsEntryRow(
                entry.PathKey, entry.Name, entry.IsDirectory, entry.SizeBytes, entry.ModifiedUtc, entry.Hidden));
            if (upserts.Count >= 20_000)
            {
                _repository.UpsertEntries(upserts, crawlGen);
                upserts.Clear();
            }
            return true;
        }, CancellationToken.None, includeHidden: true, rootHidden: rootHidden);

        if (capped)
            _repository.MarkRootStale(change.RootKey); // too big to patch live; re-crawl on next search
    }

    /// <summary>Effective hidden state for a path the watcher just saw change: its own
    /// Hidden attribute or that of any ancestor down to — but not including — the indexed
    /// root. Stopping at the root mirrors how a full crawl seeds the root as non-hidden, so
    /// watcher and crawl agree even when the root itself sits inside a hidden system folder
    /// (e.g. an indexed subtree under %AppData%). Best-effort — a lost stat means "not hidden".</summary>
    private static bool IsEffectivelyHidden(string fullPath, string rootKey)
    {
        try
        {
            for (var path = fullPath; path is not null; path = Path.GetDirectoryName(path))
            {
                if (PathKey.Canonicalize(path) == rootKey)
                    break; // reached the indexed root; its own hidden state doesn't count
                if ((File.GetAttributes(path) & FileAttributes.Hidden) != 0)
                    return true;
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
        }
        return false;
    }

    public void Dispose()
    {
        _drainTimer.Dispose();
        lock (_lock)
        {
            foreach (var entry in _watchers.Values)
                entry.Watcher.Dispose();
            _watchers.Clear();
        }
    }
}
