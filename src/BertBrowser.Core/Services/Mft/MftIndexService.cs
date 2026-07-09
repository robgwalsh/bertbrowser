using System.Collections.Concurrent;
using System.Text;
using BertBrowser.Core.Data;
using BertBrowser.Core.Interop;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Mft;

public interface IMftIndexService : IDisposable
{
    /// <summary>Begins indexing every fixed NTFS volume on a background thread each, then
    /// tailing its USN journal. Safe to call once at startup; a no-op if already started.</summary>
    void Start();

    /// <summary>True once at least one volume's initial enumeration has completed — i.e. a
    /// global search has something to hit.</summary>
    bool AnyIndexed { get; }

    /// <summary>True while any volume's initial enumeration is still running (global results
    /// are partial and a refresh is coming).</summary>
    bool IsBuilding { get; }

    /// <summary>True if <paramref name="pathKey"/> sits on a volume whose live MFT index is
    /// complete. Search uses this to treat that root as fresh and skip the crawl fallback.</summary>
    bool IsIndexed(string pathKey);

    /// <summary>Raised (on a worker thread) with a volume root key when its initial index
    /// completes, so open searches can re-query the now-populated index.</summary>
    event Action<string>? IndexRefreshed;

    /// <summary>Raised (on a worker thread) whenever <see cref="StatusText"/> changes.</summary>
    event Action? StatusChanged;

    /// <summary>Human-readable indexing state for the status bar; empty when idle.</summary>
    string StatusText { get; }
}

/// <summary>
/// Owns one <see cref="MftVolumeIndexer"/> per fixed NTFS volume: enumerates the drives,
/// builds each index off-thread, and keeps a dedicated background thread tailing each USN
/// journal for the life of the app. This is the primary index producer; the lazy
/// <c>IndexCrawler</c> remains only for roots no NTFS volume covers (network / non-NTFS).
/// </summary>
public sealed class MftIndexService : IMftIndexService
{
    private readonly FsIndexRepository _repository;
    private readonly DirSizeRepository _dirSizeRepository;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly List<Thread> _threads = new();
    private readonly List<MftVolumeIndexer> _indexers = new();
    private readonly ConcurrentDictionary<string, byte> _completedRoots = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, byte> _building = new(StringComparer.Ordinal);
    private int _started;

    public event Action<string>? IndexRefreshed;
    public event Action? StatusChanged;

    public MftIndexService(FsIndexRepository repository, DirSizeRepository dirSizeRepository)
    {
        _repository = repository;
        _dirSizeRepository = dirSizeRepository;
    }

    public bool AnyIndexed => !_completedRoots.IsEmpty;

    public bool IsBuilding => !_building.IsEmpty;

    public string StatusText { get; private set; } = "";

    public bool IsIndexed(string pathKey)
    {
        foreach (var root in _completedRoots.Keys)
        {
            if (pathKey.Equals(root, StringComparison.Ordinal) || PathKey.IsUnder(pathKey, root))
                return true;
        }
        return false;
    }

    public void Start()
    {
        if (Interlocked.Exchange(ref _started, 1) != 0)
            return;

        foreach (var drive in EnumerateNtfsVolumes())
        {
            var indexer = new MftVolumeIndexer(_repository, _dirSizeRepository, drive);
            _indexers.Add(indexer);

            var thread = new Thread(() => RunVolume(indexer, drive))
            {
                IsBackground = true,
                Name = $"mft-index-{drive}",
            };
            _threads.Add(thread);
            thread.Start();
        }
    }

    private void RunVolume(MftVolumeIndexer indexer, string drive)
    {
        var ct = _lifetime.Token;
        try
        {
            if (!indexer.Open())
                return; // couldn't open volume / no journal — leave to the crawl fallback

            _building[drive] = 0;
            UpdateStatus();

            indexer.BuildInitialIndex(ct);
            if (ct.IsCancellationRequested)
                return;

            _completedRoots[indexer.RootKey] = 0;
            _building.TryRemove(drive, out _);
            UpdateStatus();
            IndexRefreshed?.Invoke(indexer.RootKey);

            indexer.Tail(ct);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception)
        {
            // A volume that fails to index is non-fatal: searches on it fall back to a crawl.
        }
        finally
        {
            _building.TryRemove(drive, out _);
            UpdateStatus();
        }
    }

    private void UpdateStatus()
    {
        var building = _building.Keys.OrderBy(d => d, StringComparer.Ordinal).ToList();
        StatusText = building.Count switch
        {
            0 => "",
            1 => $"Indexing {building[0]}:…",
            _ => $"Indexing {string.Join(", ", building.Select(d => d + ":"))}…",
        };
        StatusChanged?.Invoke();
    }

    private static IEnumerable<string> EnumerateNtfsVolumes()
    {
        var mask = NtfsNative.GetLogicalDrives();
        for (var i = 0; i < 26; i++)
        {
            if ((mask & (1u << i)) == 0)
                continue;

            var letter = (char)('A' + i);
            var root = $"{letter}:\\";
            if (NtfsNative.GetDriveTypeW(root) != NtfsNative.DriveFixed)
                continue;
            if (IsNtfs(root))
                yield return letter.ToString();
        }
    }

    private static bool IsNtfs(string root)
    {
        var fsName = new StringBuilder(16);
        if (!NtfsNative.GetVolumeInformationW(root, IntPtr.Zero, 0, out _, out _, out _, fsName, fsName.Capacity))
            return false;
        return fsName.ToString().Equals("NTFS", StringComparison.OrdinalIgnoreCase);
    }

    public void Dispose()
    {
        _lifetime.Cancel();
        foreach (var indexer in _indexers)
            indexer.Dispose();
        _lifetime.Dispose();
    }
}
