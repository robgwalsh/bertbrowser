using System.IO.Enumeration;
using BertStat.Core.Data;
using BertStat.Core.Models;
using BertStat.Core.Paths;

namespace BertStat.Core.Services;

public sealed record DirScanProgress(string CurrentDirectory, int DirectoriesScanned, long BytesSoFar);

public interface IDirectorySizeService
{
    /// <summary>
    /// Recursively computes the content size of <paramref name="root"/>, caching results for
    /// the root AND every descendant directory. Returns the root's result, or null if cancelled.
    /// </summary>
    Task<DirSizeResult?> ComputeAsync(string root, CancellationToken ct, IProgress<DirScanProgress>? progress = null);
}

public sealed class DirectorySizeService : IDirectorySizeService
{
    private readonly DirSizeRepository _repository;
    private readonly SemaphoreSlim _concurrency = new(2);

    public DirectorySizeService(DirSizeRepository repository) => _repository = repository;

    public async Task<DirSizeResult?> ComputeAsync(
        string root, CancellationToken ct, IProgress<DirScanProgress>? progress = null)
    {
        try
        {
            await _concurrency.WaitAsync(ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return null;
        }

        try
        {
            return await Task.Run(() => Compute(root, ct, progress), ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            return null; // cancelled: write nothing, cache keeps previous values
        }
        finally
        {
            _concurrency.Release();
        }
    }

    private sealed class Frame
    {
        public required string Path;
        public required string Key;
        public Frame? Parent;
        public long Bytes;
        public int Files;
        public int Dirs;          // direct + descendant dirs
        public bool Incomplete;
        public List<Frame>? PendingChildren;
        public bool Expanded;
    }

    private DirSizeResult Compute(string root, CancellationToken ct, IProgress<DirScanProgress>? progress)
    {
        var computedUtc = DateTime.UtcNow;
        var results = new List<DirSizeResult>();
        var scanned = 0;
        long totalBytes = 0;

        var rootFrame = new Frame { Path = PathKey.NormalizeDisplay(root), Key = PathKey.Canonicalize(root) };
        var stack = new Stack<Frame>();
        stack.Push(rootFrame);

        // Iterative post-order DFS: first visit enumerates the directory and pushes child
        // frames; second visit (Expanded == true) folds the finished frame into its parent.
        while (stack.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            var frame = stack.Peek();

            if (!frame.Expanded)
            {
                frame.Expanded = true;
                EnumerateOne(frame);
                if (frame.PendingChildren is { Count: > 0 })
                {
                    foreach (var child in frame.PendingChildren)
                        stack.Push(child);
                }
                continue;
            }

            stack.Pop();
            scanned++;
            totalBytes += frame.Bytes;

            results.Add(new DirSizeResult(frame.Key, frame.Bytes, frame.Files, frame.Dirs, frame.Incomplete, computedUtc));

            if (frame.Parent is { } parent)
            {
                parent.Bytes += frame.Bytes;
                parent.Files += frame.Files;
                parent.Dirs += frame.Dirs + 1;
                parent.Incomplete |= frame.Incomplete;
            }

            if ((scanned & 0xFF) == 0)
                progress?.Report(new DirScanProgress(frame.Path, scanned, totalBytes));
        }

        _repository.UpsertMany(results);

        progress?.Report(new DirScanProgress(rootFrame.Path, scanned, rootFrame.Bytes));
        return new DirSizeResult(rootFrame.Key, rootFrame.Bytes, rootFrame.Files, rootFrame.Dirs, rootFrame.Incomplete, computedUtc);
    }

    private static void EnumerateOne(Frame frame)
    {
        try
        {
            // Skip reparse-point directories (junctions/symlinks): prevents cycles and
            // double-counting. File sizes come straight from the find data — no extra stats.
            var enumerable = new FileSystemEnumerable<(bool IsFile, string? ChildPath, string? ChildName, long Length)>(
                frame.Path,
                (ref FileSystemEntry entry) =>
                    entry.IsDirectory
                        ? (entry.Attributes & FileAttributes.ReparsePoint) == 0
                            ? (false, entry.ToFullPath(), entry.FileName.ToString(), 0L)
                            : (false, null, null, 0L)
                        : (true, null, null, entry.Length),
                new EnumerationOptions
                {
                    IgnoreInaccessible = false,
                    AttributesToSkip = 0,
                    RecurseSubdirectories = false,
                });

            foreach (var (isFile, childPath, childName, length) in enumerable)
            {
                if (isFile)
                {
                    frame.Files++;
                    frame.Bytes += length;
                }
                else if (childPath is not null)
                {
                    (frame.PendingChildren ??= new List<Frame>()).Add(new Frame
                    {
                        Path = childPath,
                        Key = frame.Key.TrimEnd('\\') + '\\' + childName!.ToUpperInvariant(),
                        Parent = frame,
                    });
                }
            }
        }
        catch (UnauthorizedAccessException)
        {
            MarkIncompleteUpwards(frame);
        }
        catch (IOException)
        {
            MarkIncompleteUpwards(frame);
        }
    }

    private static void MarkIncompleteUpwards(Frame frame)
    {
        // Incomplete propagates to ancestors when folding, but mark eagerly too so a
        // partially-enumerated frame is flagged even if it has no children to fold.
        for (Frame? f = frame; f is not null; f = f.Parent)
            f.Incomplete = true;
    }
}
