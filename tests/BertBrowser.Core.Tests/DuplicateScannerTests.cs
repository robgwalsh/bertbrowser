using System.Collections.Concurrent;
using System.Security.Cryptography;
using System.Text;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Duplicates;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The three passes, against a fake index and a fake hasher. Both seams exist for this: contriving
/// two files that share their first 64 KB and differ after it — or three names for one file on
/// disk — takes a line here and a privileged filesystem layout otherwise.
/// </summary>
public sealed class DuplicateScannerTests
{
    private const long Head = DuplicateRules.HeadSampleBytes;

    private static readonly DuplicateScanRequest Anywhere =
        new(RootPath: null, MinSizeBytes: 1, IncludeHidden: true, SkipSystemFolders: false);

    /// <summary>Content of <paramref name="length"/> bytes filled from <paramref name="seed"/>, so
    /// two files agree exactly as far as their seeds do.</summary>
    private static byte[] Content(int length, string seed)
    {
        var pattern = Encoding.UTF8.GetBytes(seed);
        var bytes = new byte[length];
        for (var i = 0; i < length; i++) bytes[i] = pattern[i % pattern.Length];
        return bytes;
    }

    /// <summary>Content that agrees with <paramref name="seed"/> for the whole sample and then
    /// diverges — the case the second hashing pass exists for.</summary>
    private static byte[] SameHead(int length, string seed, string tail)
    {
        var bytes = Content(length, seed);
        var divergence = Encoding.UTF8.GetBytes(tail);
        for (var i = 0; i < divergence.Length; i++) bytes[(int)Head + i] = divergence[i];
        return bytes;
    }

    private static (DuplicateScanner Scanner, FakeHasher Hasher) Build(
        Dictionary<string, byte[]> files,
        Dictionary<string, FileIdentity>? identities = null,
        IEnumerable<string>? unreadable = null,
        Action<FakeHasher, string, long>? afterChunk = null)
    {
        var hasher = new FakeHasher(files, identities, unreadable, afterChunk);
        var source = new FakeCandidateSource(files);
        return (new DuplicateScanner(source, hasher), hasher);
    }

    // --- the ordinary cases ---

    [Fact]
    public void IdenticalFiles_AreAGroup()
    {
        var (scanner, _) = Build(new()
        {
            [@"C:\a\report.pdf"] = Content(4096, "same"),
            [@"C:\b\report.pdf"] = Content(4096, "same"),
        });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        var group = Assert.Single(outcome.Groups);
        Assert.Equal(2, group.Files.Count);
        Assert.Equal(4096, group.SizeBytes);
        Assert.Equal(4096, group.WastedBytes);
    }

    /// <summary>
    /// The shortlist is a size collision and nothing more. Make the sampling pass accept on size
    /// alone and this goes red — which is the bug that would offer to delete two unrelated files
    /// that happen to be the same length.
    /// </summary>
    [Fact]
    public void SameSizeDifferentContent_IsNotAGroup()
    {
        var (scanner, _) = Build(new()
        {
            [@"C:\a\one.bin"] = Content(4096, "aaaa"),
            [@"C:\b\two.bin"] = Content(4096, "bbbb"),
        });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Empty(outcome.Groups);
    }

    /// <summary>
    /// The whole reason there is a third pass. These two agree for every byte the sample reads and
    /// differ immediately after it — a disk image, a container with a shared header, a VM file.
    /// Drop the full hash and this goes red.
    /// </summary>
    [Fact]
    public void SameHeadDifferentTail_IsNotAGroup()
    {
        var length = (int)Head + 4096;
        var (scanner, hasher) = Build(new()
        {
            [@"C:\a\disk.img"] = SameHead(length, "same", "LEFT"),
            [@"C:\b\disk.img"] = SameHead(length, "same", "RIGHT"),
        });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Empty(outcome.Groups);

        // And it really did have to read them whole to find out.
        Assert.Equal(2, hasher.Calls.Count(c => c.MaxBytes == 0));
    }

    /// <summary>
    /// A file the sample read to its end was hashed in full by it, so asking again would read every
    /// small file on the shortlist twice for nothing.
    /// </summary>
    [Fact]
    public void FilesSmallerThanTheSample_AreNeverReadTwice()
    {
        var (scanner, hasher) = Build(new()
        {
            [@"C:\a\note.txt"] = Content(2048, "same"),
            [@"C:\b\note.txt"] = Content(2048, "same"),
        });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Single(outcome.Groups);
        Assert.DoesNotContain(hasher.Calls, c => c.MaxBytes == 0);
    }

    /// <summary>
    /// A row can be stale: the file shrank after it was indexed. Grouping on the indexed length
    /// would compare it against one that did not, so the real byte count read is what counts.
    /// </summary>
    [Fact]
    public void GroupsCarryTheLengthActuallyRead_NotTheIndexedOne()
    {
        var (scanner, _) = Build(new()
        {
            [@"C:\a\x.bin"] = Content(1024, "same"),
            [@"C:\b\x.bin"] = Content(1024, "same"),
        });

        // The index still thinks both are 9999 bytes.
        var outcome = scanner.Scan(
            Anywhere with { MinSizeBytes = 1 }, isBuilding: false, isIndexed: true);

        var group = Assert.Single(outcome.Groups);
        Assert.Equal(1024, group.SizeBytes);
        Assert.All(group.Files, f => Assert.Equal(1024, f.SizeBytes));
    }

    // --- hardlinks ---

    /// <summary>
    /// Two names for one file are not two copies: deleting one frees nothing. They fold into a
    /// single entry that carries the other name.
    /// </summary>
    [Fact]
    public void HardlinkedCopies_AreOneEntry()
    {
        var identity = new FileIdentity(1, 42);
        var (scanner, _) = Build(
            new()
            {
                [@"C:\a\x.bin"] = Content(4096, "same"),
                [@"C:\b\x.bin"] = Content(4096, "same"),
                [@"C:\c\x.bin"] = Content(4096, "same"),
            },
            identities: new()
            {
                [@"C:\a\x.bin"] = identity,
                [@"C:\b\x.bin"] = identity,
            });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        var group = Assert.Single(outcome.Groups);
        Assert.Equal(2, group.Files.Count);

        var folded = Assert.Single(group.Files, f => f.HardlinkPaths.Count > 0);
        Assert.Equal(@"C:\a\x.bin", folded.DisplayPath);
        Assert.Equal([@"C:\b\x.bin"], folded.HardlinkPaths);

        // And the waste is one redundant copy, not two.
        Assert.Equal(4096, group.WastedBytes);
    }

    /// <summary>
    /// C:\Windows\WinSxS is built almost entirely this way. Every "copy" being one file means
    /// there is nothing to reclaim and nothing to show — reporting the group would bury every real
    /// result under millions of rows that free nothing.
    /// </summary>
    [Fact]
    public void WhenEveryCopyIsOneFile_ThereIsNoGroup()
    {
        var identity = new FileIdentity(1, 7);
        var (scanner, _) = Build(
            new()
            {
                [@"C:\w\a.dll"] = Content(4096, "same"),
                [@"C:\w\b.dll"] = Content(4096, "same"),
                [@"C:\w\c.dll"] = Content(4096, "same"),
            },
            identities: new()
            {
                [@"C:\w\a.dll"] = identity,
                [@"C:\w\b.dll"] = identity,
                [@"C:\w\c.dll"] = identity,
            });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Empty(outcome.Groups);
    }

    // --- availability ---

    /// <summary>
    /// The sizeless build path writes every row with size_bytes = 0, so every file collides with
    /// every other. Trusting that would mean reading an entire disk to discover nothing. Make
    /// Classify return Ready for this shape and the assertion on the hasher goes red.
    /// </summary>
    [Fact]
    public void NoSizeData_HashesNothingAtAll()
    {
        var hasher = new FakeHasher(new Dictionary<string, byte[]>(), null, null, null);
        var source = new FakeCandidateSource(new Dictionary<string, byte[]>())
        {
            FilesInScope = 5000,
            SizedFilesInScope = 0,
        };

        var outcome = new DuplicateScanner(source, hasher)
            .Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Equal(DuplicateScanAvailability.NoSizeData, outcome.Availability);
        Assert.Empty(outcome.Groups);
        Assert.Empty(hasher.Calls);
    }

    [Fact]
    public void NotIndexed_HashesNothingAtAll()
    {
        var hasher = new FakeHasher(new Dictionary<string, byte[]>(), null, null, null);
        var source = new FakeCandidateSource(new Dictionary<string, byte[]>());

        var outcome = new DuplicateScanner(source, hasher)
            .Scan(Anywhere, isBuilding: false, isIndexed: false);

        Assert.Equal(DuplicateScanAvailability.NotIndexed, outcome.Availability);
        Assert.Empty(hasher.Calls);
    }

    // --- failures ---

    /// <summary>
    /// One file's failure never costs the others — the rule every executor in this app holds to.
    /// The result is a floor and says so.
    /// </summary>
    [Fact]
    public void AnUnreadableCandidate_IsSkippedAndSaidSo()
    {
        var (scanner, _) = Build(
            new()
            {
                [@"C:\a\x.bin"] = Content(4096, "same"),
                [@"C:\b\x.bin"] = Content(4096, "same"),
                [@"C:\c\x.bin"] = Content(4096, "same"),
            },
            unreadable: [@"C:\c\x.bin"]);

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.True(outcome.Incomplete);
        var group = Assert.Single(outcome.Groups);
        Assert.Equal(2, group.Files.Count);
    }

    // --- cancellation ---

    /// <summary>
    /// A cancel during sampling must not leave a group behind whose members only ever agreed on
    /// their first 64 KB. These two would be exactly that: same head, different tail.
    /// </summary>
    [Fact]
    public void CancelDuringSampling_ReportsNothingItHadNotConfirmed()
    {
        var length = (int)Head + 4096;
        using var cts = new CancellationTokenSource();

        var (scanner, _) = Build(
            new()
            {
                [@"C:\a\disk.img"] = SameHead(length, "same", "LEFT"),
                [@"C:\b\disk.img"] = SameHead(length, "same", "RIGHT"),
            },
            afterChunk: (_, _, _) => cts.Cancel());

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true, cts.Token);

        Assert.True(outcome.Cancelled);
        Assert.Empty(outcome.Groups);
    }

    /// <summary>
    /// A cancel during the full pass keeps what the sample had already settled: those files were
    /// read end to end, so their verdict is not provisional. Handing back nothing would throw away
    /// work that is genuinely finished.
    /// </summary>
    [Fact]
    public void CancelDuringTheFullPass_KeepsWhatTheSampleSettled()
    {
        var large = (int)Head + 8192;
        using var cts = new CancellationTokenSource();

        var (scanner, _) = Build(
            new()
            {
                // Settled by sampling: read to the end, so already final.
                [@"C:\a\note.txt"] = Content(2048, "small"),
                [@"C:\b\note.txt"] = Content(2048, "small"),

                // Only ever sampled; the full pass is what gets cancelled.
                [@"C:\a\disk.img"] = SameHead(large, "same", "LEFT"),
                [@"C:\b\disk.img"] = SameHead(large, "same", "RIGHT"),
            },
            // Only a full read ever gets past the sample's length.
            afterChunk: (_, _, soFar) => { if (soFar > Head) cts.Cancel(); });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true, cts.Token);

        Assert.True(outcome.Cancelled);
        var group = Assert.Single(outcome.Groups);
        Assert.Equal(2048, group.SizeBytes);
    }

    // --- progress ---

    [Fact]
    public void ProgressWalksThroughThePhases()
    {
        var length = (int)Head + 4096;
        var reports = new SynchronousProgress<DuplicateScanProgress>();

        var (scanner, _) = Build(new()
        {
            [@"C:\a\disk.img"] = Content(length, "same"),
            [@"C:\b\disk.img"] = Content(length, "same"),
        });

        scanner.Scan(Anywhere, isBuilding: false, isIndexed: true, CancellationToken.None, reports);

        var phases = reports.Reports.Select(r => r.Phase).Distinct().ToList();
        Assert.Contains(DuplicateScanPhase.Shortlisting, phases);
        Assert.Contains(DuplicateScanPhase.Sampling, phases);
        Assert.Contains(DuplicateScanPhase.Hashing, phases);

        // Sampling's total is exact and reads only the head of each file; the full pass reads both
        // whole. A bar with no denominator is what an indeterminate one is for, and neither of
        // these needs to be one.
        var sampling = reports.Reports.Last(r => r.Phase == DuplicateScanPhase.Sampling);
        Assert.Equal(Head * 2, sampling.BytesTotal);

        var hashing = reports.Reports.Last(r => r.Phase == DuplicateScanPhase.Hashing);
        Assert.Equal(length * 2L, hashing.BytesTotal);
    }

    // --- ordering ---

    /// <summary>Biggest saving first: the list is read top-down and the point of it is space.</summary>
    [Fact]
    public void GroupsComeBackWorstFirst()
    {
        var (scanner, _) = Build(new()
        {
            [@"C:\a\small.bin"] = Content(1024, "s"),
            [@"C:\b\small.bin"] = Content(1024, "s"),
            [@"C:\a\big.bin"] = Content(8192, "b"),
            [@"C:\b\big.bin"] = Content(8192, "b"),
        });

        var outcome = scanner.Scan(Anywhere, isBuilding: false, isIndexed: true);

        Assert.Equal(2, outcome.Groups.Count);
        Assert.Equal(8192, outcome.Groups[0].SizeBytes);
        Assert.Equal(9216, outcome.WastedBytes);
    }

    // --- meta ---

    /// <summary>
    /// Guards the guard. If the fake hasher were to answer the same digest for different bytes,
    /// every separation test above would pass for the wrong reason.
    /// </summary>
    [Fact]
    public void TheContentCheck_ActuallySeparatesFilesDifferingByOneByte()
    {
        var left = Content(4096, "same");
        var right = Content(4096, "same");
        right[4095] ^= 0xFF;

        var (scanner, _) = Build(new()
        {
            [@"C:\a\x.bin"] = left,
            [@"C:\b\x.bin"] = right,
        });

        Assert.Empty(scanner.Scan(Anywhere, isBuilding: false, isIndexed: true).Groups);
    }
}

/// <summary>
/// An in-memory index: the files it knows about, and — like the real repository — only the ones
/// sharing a byte length with another. The two scope counts default to describing a healthy index
/// and are settable for the availability cases.
/// </summary>
internal sealed class FakeCandidateSource : IDuplicateCandidateSource
{
    private readonly Dictionary<string, byte[]> _files;

    public FakeCandidateSource(Dictionary<string, byte[]> files)
    {
        _files = files;
        FilesInScope = files.Count;
        SizedFilesInScope = files.Count;
    }

    public int FilesInScope { get; set; }
    public int SizedFilesInScope { get; set; }

    public DuplicateShortlist Shortlist(DuplicateScanRequest request, CancellationToken ct)
    {
        var candidates = _files
            .GroupBy(f => (long)f.Value.Length)
            .Where(g => g.Count() > 1)
            .SelectMany(g => g)
            .OrderBy(f => f.Key, StringComparer.Ordinal)
            .Select(f => new SearchHit(
                f.Key,
                Path.GetDirectoryName(f.Key) ?? "",
                Path.GetFileName(f.Key),
                false,
                f.Value.Length,
                new DateTime(2024, 3, 14, 9, 26, 53, DateTimeKind.Utc)))
            .ToList();

        return new DuplicateShortlist(candidates, FilesInScope, SizedFilesInScope);
    }
}

/// <summary>
/// A hasher that really hashes, but of bytes held in memory and in a fixed number of chunks with a
/// hook between them — so a cancel can be made to land in the middle of a named file every time,
/// which is what a real multi-gigabyte fixture would otherwise be needed for.
/// </summary>
internal sealed class FakeHasher(
    Dictionary<string, byte[]> files,
    Dictionary<string, FileIdentity>? identities,
    IEnumerable<string>? unreadable,
    Action<FakeHasher, string, long>? afterChunk) : IFileHasher
{
    private const int ChunkBytes = 4096;

    private readonly HashSet<string> _unreadable =
        new(unreadable ?? [], StringComparer.OrdinalIgnoreCase);

    public ConcurrentBag<(string Path, long MaxBytes)> Calls { get; } = [];

    public FileFingerprint? Hash(string path, long maxBytes, Action<long>? progress, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();
        Calls.Add((path, maxBytes));

        if (_unreadable.Contains(path)) return null;
        if (!files.TryGetValue(path, out var content)) return null;

        var limit = maxBytes > 0 ? (int)Math.Min(maxBytes, content.Length) : content.Length;

        var read = 0;
        while (read < limit)
        {
            ct.ThrowIfCancellationRequested();

            var chunk = Math.Min(ChunkBytes, limit - read);
            read += chunk;
            progress?.Invoke(chunk);
            afterChunk?.Invoke(this, path, read);
        }

        var hash = Convert.ToHexString(SHA256.HashData(content.AsSpan(0, limit)));
        return new FileFingerprint(
            hash,
            limit,
            identities is not null && identities.TryGetValue(path, out var id) ? id : null);
    }
}
