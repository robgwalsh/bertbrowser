using BertBrowser.Core.Services.Compare;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class FileContentComparerTests : IDisposable
{
    private readonly string _root;
    private readonly FileContentComparer _comparer = new();

    public FileContentComparerTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-content-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        try
        {
            Directory.Delete(_root, recursive: true);
        }
        catch (IOException)
        {
            // A file someone else still holds open is not this test's problem.
        }
    }

    private string File_(byte[] content, string name)
    {
        var path = Path.Combine(_root, name);
        File.WriteAllBytes(path, content);
        return path;
    }

    private static byte[] Bytes(int count, byte seed = 0)
    {
        var bytes = new byte[count];
        for (var i = 0; i < count; i++) bytes[i] = (byte)((i + seed) % 251);
        return bytes;
    }

    private ContentComparison Compare(string a, string b) =>
        _comparer.Compare(a, b, progress: null, CancellationToken.None);

    // --- the verdicts ---

    [Fact]
    public void TwoIdenticalFilesAreIdentical()
    {
        var content = Bytes(5000);
        var result = Compare(File_(content, "a.bin"), File_(content, "b.bin"));

        Assert.Equal(ContentVerdict.Identical, result.Verdict);
        Assert.Null(result.FirstDifferenceOffset);
        Assert.Equal(5000, result.LeftBytes);
    }

    [Fact]
    public void TwoEmptyFilesAreIdentical() =>
        Assert.Equal(ContentVerdict.Identical, Compare(File_([], "a.bin"), File_([], "b.bin")).Verdict);

    [Fact]
    public void AOneByteChangeReportsThatExactOffset()
    {
        var left = Bytes(5000);
        var right = Bytes(5000);
        right[1234] ^= 0xFF;

        var result = Compare(File_(left, "a.bin"), File_(right, "b.bin"));

        Assert.Equal(ContentVerdict.Differs, result.Verdict);
        Assert.Equal(1234, result.FirstDifferenceOffset);
    }

    /// <summary>
    /// The offset must be exact across a buffer boundary too — narrowing to the byte happens inside
    /// a chunk, and an off-by-one there would point at the wrong place in a large file.
    /// </summary>
    [Fact]
    public void ADifferencePastTheFirstBufferReportsItsAbsoluteOffset()
    {
        var left = Bytes(3_000_000);
        var right = (byte[])left.Clone();
        right[2_500_000] ^= 0xFF;

        Assert.Equal(2_500_000, Compare(File_(left, "a.bin"), File_(right, "b.bin")).FirstDifferenceOffset);
    }

    /// <summary>
    /// "Differs at byte 4,096" is a more useful answer than "the sizes differ" — it says the shorter
    /// one is a prefix, which is what a truncated download looks like.
    /// </summary>
    [Fact]
    public void APrefixReportsWhereTheShorterOneEnded()
    {
        var longer = Bytes(6000);
        var shorter = longer[..4096];

        var result = Compare(File_(longer, "a.bin"), File_(shorter, "b.bin"));

        Assert.Equal(ContentVerdict.Differs, result.Verdict);
        Assert.Equal(4096, result.FirstDifferenceOffset);
        Assert.Equal(6000, result.LeftBytes);
        Assert.Equal(4096, result.RightBytes);
    }

    [Fact]
    public void AnEmptyFileAgainstANonEmptyOneDiffersAtZero()
    {
        var result = Compare(File_([], "a.bin"), File_(Bytes(10), "b.bin"));

        Assert.Equal(ContentVerdict.Differs, result.Verdict);
        Assert.Equal(0, result.FirstDifferenceOffset);
    }

    [Fact]
    public void FilesDifferingOnlyInTheirLastByteAreCaught()
    {
        var left = Bytes(1000);
        var right = (byte[])left.Clone();
        right[^1] ^= 0xFF;

        Assert.Equal(999, Compare(File_(left, "a.bin"), File_(right, "b.bin")).FirstDifferenceOffset);
    }

    // --- the rule that matters ---

    /// <summary>
    /// The whole safety argument. A comparison's "same" is what lets a sync delete something, so a
    /// side that could not be read must never come back as a match.
    /// </summary>
    [Fact]
    public void AnUnreadableSideIsNeverIdentical()
    {
        var real = File_(Bytes(10), "a.bin");
        var missing = Path.Combine(_root, "nope.bin");

        Assert.Equal(ContentVerdict.Unreadable, Compare(missing, real).Verdict);
        Assert.Equal(ContentVerdict.Unreadable, Compare(real, missing).Verdict);
    }

    [Fact]
    public void ADirectoryIsUnreadable_NotAThrow() =>
        Assert.Equal(ContentVerdict.Unreadable, Compare(_root, File_(Bytes(10), "a.bin")).Verdict);

    /// <summary>
    /// The sharing rule again: this app must be able to compare a file it is itself writing, or a
    /// comparison would block the very operations it exists to check.
    /// </summary>
    [Fact]
    public void AFileSomeoneElseHasOpenForWritingIsStillComparable()
    {
        var content = Bytes(500);
        var a = File_(content, "a.bin");
        var b = File_(content, "b.bin");

        using var holder = new FileStream(a, FileMode.Open, FileAccess.ReadWrite, FileShare.ReadWrite | FileShare.Delete);

        Assert.Equal(ContentVerdict.Identical, Compare(a, b).Verdict);
    }

    // --- the short circuit ---

    /// <summary>
    /// The same path twice is the same file, which a handle already knows. Reading both sides to
    /// confirm it would be minutes spent learning what Windows said immediately — and the progress
    /// callback never firing is how this test can tell the difference.
    /// </summary>
    [Fact]
    public void ThePathComparedWithItselfIsIdenticalWithoutReading()
    {
        var path = File_(Bytes(2_000_000), "a.bin");

        var read = 0L;
        var result = _comparer.Compare(path, path, delta => read += delta, CancellationToken.None);

        Assert.Equal(ContentVerdict.Identical, result.Verdict);
        Assert.Equal(0, read);
    }

    // --- progress and cancellation ---

    [Fact]
    public void ProgressAddsUpToWhatWasRead()
    {
        var content = Bytes(2_500_000);
        var a = File_(content, "a.bin");
        var b = File_(content, "b.bin");

        var read = 0L;
        _comparer.Compare(a, b, delta => read += delta, CancellationToken.None);

        Assert.Equal(content.Length, read);
    }

    [Fact]
    public void ACancelledTokenThrows()
    {
        var content = Bytes(100);
        var a = File_(content, "a.bin");
        var b = File_(content, "b.bin");

        using var cts = new CancellationTokenSource();
        cts.Cancel();

        Assert.Throws<OperationCanceledException>(() => _comparer.Compare(a, b, null, cts.Token));
    }
}
