using BertBrowser.Core.Data;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services;

/// <summary>
/// Builds the fs_entry search index for a subtree. Entries are written in bounded
/// chunks (one transaction each) so a multi-million-entry crawl never buffers the
/// whole tree in memory and never starves other writers with one giant transaction.
/// A cancelled crawl leaves any rows it already wrote (harmless — they carry real
/// data) but never sweeps or marks the root complete.
/// </summary>
public sealed class IndexCrawler
{
    private const int ChunkSize = 20_000;

    private readonly FsIndexRepository _repository;
    private readonly SemaphoreSlim _concurrency = new(2);

    public IndexCrawler(FsIndexRepository repository) => _repository = repository;

    /// <summary>
    /// Crawls <paramref name="root"/> into the index. Returns true when the crawl
    /// completed and the root was registered as complete and fresh.
    /// </summary>
    public async Task<bool> CrawlAsync(string root, CancellationToken ct, IProgress<int>? progress = null)
    {
        try
        {
            await _concurrency.WaitAsync(ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return false;
        }

        try
        {
            return await Task.Run(() => Crawl(root, ct, progress), ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return false;
        }
        finally
        {
            _concurrency.Release();
        }
    }

    private bool Crawl(string root, CancellationToken ct, IProgress<int>? progress)
    {
        var rootKey = PathKey.Canonicalize(root);
        var rootDisplay = PathKey.NormalizeDisplay(root);
        var crawledUtc = DateTime.UtcNow;
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        var buffer = new List<FsEntryRow>(ChunkSize);
        var total = 0;

        FileSystemWalker.Walk(root, entry =>
        {
            buffer.Add(new FsEntryRow(
                entry.PathKey, entry.Name, entry.IsDirectory, entry.SizeBytes, entry.ModifiedUtc, entry.Hidden));
            if (buffer.Count >= ChunkSize)
            {
                _repository.UpsertEntries(buffer, crawlGen);
                total += buffer.Count;
                buffer.Clear();
                progress?.Report(total);
            }
            return true;
        }, ct, includeHidden: true); // index everything with its hidden flag; queries filter

        _repository.UpsertEntries(buffer, crawlGen);
        total += buffer.Count;

        _repository.SweepVanished(rootKey, crawlGen);
        _repository.UpsertRoot(rootKey, rootDisplay, crawledUtc, complete: true);
        progress?.Report(total);
        return true;
    }
}
