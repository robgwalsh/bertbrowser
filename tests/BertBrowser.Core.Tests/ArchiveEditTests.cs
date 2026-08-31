using System.Text;
using BertBrowser.Core.Services.Archives;
using SharpCompress.Common;
using SharpCompress.Writers;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Editing a container in place — which is really rewriting it beside itself and swapping.
/// </summary>
/// <remarks>
/// The happy path matters least here. What these are for is the safety: that a refusal is a refusal
/// rather than a silent conversion, that a failure leaves the original exactly where it was, and
/// that undo puts back the same bytes.
/// </remarks>
public class ArchiveEditTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), $"bertbrowser-archedit-{Guid.NewGuid():N}");

    private readonly SharpCompressArchiveReader _reader = new();
    private readonly ArchiveEditPlanner _planner = new();
    private readonly ArchiveEditExecutor _executor;

    public ArchiveEditTests()
    {
        Directory.CreateDirectory(_root);
        _executor = new ArchiveEditExecutor(_reader);
    }

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { /* best effort */ }
    }

    private string Zip(string name, params (string Key, string Body)[] entries) =>
        Write(name, ArchiveType.Zip, CompressionType.Deflate, entries);

    private string Write(string name, ArchiveType type, CompressionType compression,
        params (string Key, string Body)[] entries)
    {
        var path = Path.Combine(_root, name);
        using var file = File.Create(path);
        using var writer = WriterFactory.Open(file, type, new WriterOptions(compression));
        foreach (var (key, body) in entries)
        {
            using var source = new MemoryStream(Encoding.UTF8.GetBytes(body));
            writer.Write(key, source, new DateTime(2026, 2, 3, 4, 5, 6));
        }
        return path;
    }

    private ArchiveEditPlan Plan(string archive, params ArchiveEdit[] edits) =>
        _planner.Plan(_reader.Read(archive, null), archive, new FileInfo(archive).Length, edits);

    private IReadOnlyList<string> EntriesOf(string archive) =>
        _reader.Read(archive, null).ByPath.Values
            .Where(n => !n.IsDirectory).Select(n => n.Path).Order().ToList();

    private string Body(string archive, string entry) =>
        Encoding.UTF8.GetString(_reader.ReadEntry(archive, entry, 4096, null)!);

    // --- what it does ---

    [Fact]
    public void RemovingAnEntryRewritesWithoutIt()
    {
        var zip = Zip("a.zip", ("keep.txt", "kept"), ("drop.txt", "gone"), ("src/app.js", "code"));

        var outcome = _executor.Execute(Plan(zip, new RemoveEntry("drop.txt")));

        Assert.Null(outcome.Failure);
        Assert.Equal([@"keep.txt", @"src\app.js"], EntriesOf(zip));
        Assert.Equal("kept", Body(zip, "keep.txt"));
        Assert.Equal("code", Body(zip, @"src\app.js"));
    }

    [Fact]
    public void RemovingAFolderTakesEverythingUnderIt()
    {
        var zip = Zip("a.zip", ("keep.txt", "k"), ("src/app.js", "a"), ("src/lib/util.js", "u"));

        _executor.Execute(Plan(zip, new RemoveEntry("src")));

        Assert.Equal(["keep.txt"], EntriesOf(zip));
    }

    [Fact]
    public void RenamingAnEntryKeepsItsContents()
    {
        var zip = Zip("a.zip", ("old.txt", "unchanged"), ("other.txt", "x"));

        _executor.Execute(Plan(zip, new RenameEntry("old.txt", "new.txt")));

        Assert.Equal(["new.txt", "other.txt"], EntriesOf(zip));
        Assert.Equal("unchanged", Body(zip, "new.txt"));
    }

    [Fact]
    public void RenamingAFolderMovesEverythingBeneathIt()
    {
        var zip = Zip("a.zip", ("src/app.js", "a"), ("src/lib/util.js", "u"));

        _executor.Execute(Plan(zip, new RenameEntry("src", "source")));

        Assert.Equal([@"source\app.js", @"source\lib\util.js"], EntriesOf(zip));
    }

    [Fact]
    public void AddingAFilePutsItIn()
    {
        var zip = Zip("a.zip", ("there.txt", "x"));
        var extra = Path.Combine(_root, "extra.txt");
        File.WriteAllText(extra, "brand new");

        _executor.Execute(Plan(zip, new AddFile(extra, "extra.txt")));

        Assert.Equal(["extra.txt", "there.txt"], EntriesOf(zip));
        Assert.Equal("brand new", Body(zip, "extra.txt"));
    }

    [Theory]
    [InlineData(".tar", ArchiveType.Tar, CompressionType.None)]
    [InlineData(".tar.gz", ArchiveType.Tar, CompressionType.GZip)]
    public void TarFamilyRewritesToTheSameKindOfContainer(
        string suffix, ArchiveType type, CompressionType compression)
    {
        var path = Write("a" + suffix, type, compression, ("keep.txt", "k"), ("drop.txt", "d"));

        var outcome = _executor.Execute(Plan(path, new RemoveEntry("drop.txt")));

        Assert.Null(outcome.Failure);
        Assert.Equal(["keep.txt"], EntriesOf(path));
        // Still the format it started as — a rewrite must not quietly change what the file is.
        Assert.True(_reader.Read(path, null).Ok);
    }

    // --- what it refuses, and why ---

    [Fact]
    public void A7zIsRefusedByNameRatherThanRewrittenAsSomethingElse()
    {
        var path = ArchiveFixtures.WriteTo(
            Path.Combine(_root, "x.7z"), ArchiveFixtures.PlainSevenZip);

        var plan = Plan(path, new RemoveEntry("notes.txt"));

        Assert.Equal(ArchiveEditRejection.FormatNotWritable, plan.Rejected!.Reason);
        Assert.Contains(".7z", plan.Rejected.Message);
    }

    /// <summary>A rewrite would drop the encryption, which is worse than not offering it.</summary>
    [Fact]
    public void AnEncryptedArchiveIsRefused()
    {
        var path = ArchiveFixtures.WriteTo(
            Path.Combine(_root, "enc.zip"), ArchiveFixtures.EncryptedZip);

        var plan = Plan(path, new RemoveEntry("notes.txt"));

        Assert.Equal(ArchiveEditRejection.Encrypted, plan.Rejected!.Reason);
    }

    [Fact]
    public void AnArchiveTooLargeToRewriteIsRefusedWithItsCost()
    {
        var zip = Zip("a.zip", ("x.txt", "x"));

        var plan = _planner.Plan(
            _reader.Read(zip, null), zip,
            ArchiveEditPlanner.MaxRewriteBytes + 1, [new RemoveEntry("x.txt")]);

        Assert.Equal(ArchiveEditRejection.TooLarge, plan.Rejected!.Reason);
        Assert.Contains("rewriting", plan.Rejected.Message);
    }

    [Fact]
    public void RenamingOntoATakenNameIsRefused()
    {
        var zip = Zip("a.zip", ("a.txt", "1"), ("b.txt", "2"));

        var plan = Plan(zip, new RenameEntry("a.txt", "b.txt"));

        Assert.Equal(ArchiveEditRejection.NameTaken, plan.Rejected!.Reason);
    }

    [Fact]
    public void AnIllegalNameIsRefusedInTheRenameRulesOwnWords()
    {
        var zip = Zip("a.zip", ("a.txt", "1"));

        var plan = Plan(zip, new RenameEntry("a.txt", "b?.txt"));

        Assert.Equal(ArchiveEditRejection.InvalidName, plan.Rejected!.Reason);
        Assert.Equal(Core.Services.Rename.RenamePattern.Validate("b?.txt"), plan.Rejected.Message);
    }

    [Fact]
    public void AnEntryThatIsNoLongerThereIsRefused()
    {
        var zip = Zip("a.zip", ("a.txt", "1"));

        Assert.Equal(ArchiveEditRejection.EntryMissing, Plan(zip, new RemoveEntry("gone.txt")).Rejected!.Reason);
    }

    // --- the safety invariants ---

    /// <summary>
    /// The whole reason the new container is built first. At every moment before the swap, the
    /// archive on disk is the one the user started with.
    /// </summary>
    [Fact]
    public void ACancelledEditLeavesTheOriginalExactlyAsItWas()
    {
        var zip = Zip("a.zip", ("one.txt", "1"), ("two.txt", "2"), ("three.txt", "3"));
        var before = File.ReadAllBytes(zip);

        using var cts = new CancellationTokenSource();
        cts.Cancel();

        var outcome = _executor.Execute(Plan(zip, new RemoveEntry("two.txt")), cts.Token);

        Assert.True(outcome.Cancelled);
        Assert.Equal(before, File.ReadAllBytes(zip));
        Assert.Empty(LeftoverRewrites());
    }

    [Fact]
    public void NothingIsLeftBesideTheArchiveAfterASuccessfulEdit()
    {
        var zip = Zip("a.zip", ("one.txt", "1"), ("two.txt", "2"));

        _executor.Execute(Plan(zip, new RemoveEntry("two.txt")));

        Assert.Empty(LeftoverRewrites());
    }

    /// <summary>The meta-test: the leftover check can actually fail.</summary>
    [Fact]
    public void TheLeftoverCheckNoticesAHalfWrittenContainer()
    {
        File.WriteAllText(Path.Combine(_root, "a.zip" + ArchiveEditExecutor.RewriteMarker + "-deadbeef"), "half");
        Assert.NotEmpty(LeftoverRewrites());
    }

    /// <summary>
    /// The original is held rather than erased, which is what makes the edit undoable — and it is
    /// still there afterwards, because only RetireUndoable ever commits it.
    /// </summary>
    [Fact]
    public void TheReplacedArchiveIsHeldRatherThanErased()
    {
        var zip = Zip("a.zip", ("one.txt", "1"), ("two.txt", "2"));

        var outcome = _executor.Execute(Plan(zip, new RemoveEntry("two.txt")));

        Assert.True(outcome.CanUndo);
        Assert.NotNull(outcome.StagedOriginal);
        Assert.True(File.Exists(outcome.StagedOriginal));
    }

    [Fact]
    public void UndoPutsBackTheSameBytes()
    {
        var zip = Zip("a.zip", ("one.txt", "1"), ("two.txt", "2"));
        var before = File.ReadAllBytes(zip);

        var outcome = _executor.Execute(Plan(zip, new RemoveEntry("two.txt")));
        Assert.Equal(["one.txt"], EntriesOf(zip));

        Assert.Null(_executor.Undo(outcome));
        Assert.Equal(before, File.ReadAllBytes(zip));
        Assert.Equal(["one.txt", "two.txt"], EntriesOf(zip));
    }

    /// <summary>
    /// Committing is what finally erases the held original, and it is the only thing that does —
    /// so the replaced container outlives its undo record by exactly one operation.
    /// </summary>
    [Fact]
    public void CommittingErasesTheHeldOriginal()
    {
        var zip = Zip("a.zip", ("one.txt", "1"), ("two.txt", "2"));
        var outcome = _executor.Execute(Plan(zip, new RemoveEntry("two.txt")));

        ArchiveEditExecutor.CommitStaging(outcome);

        Assert.False(File.Exists(outcome.StagedOriginal));
        Assert.True(File.Exists(zip));
    }

    /// <summary>
    /// The guard on the only erase in this file. Hand it something not named the way this class
    /// names staged originals and it must do nothing — mutate the check to return true and this
    /// deletes a real archive.
    /// </summary>
    [Fact]
    public void CommittingRefusesAnythingThatIsNotAHeldOriginal()
    {
        var innocent = Zip("precious.zip", ("mine.txt", "do not delete"));

        ArchiveEditExecutor.CommitStaging(
            new ArchiveEditOutcome("whatever.zip", innocent, 1, null, false));

        Assert.True(File.Exists(innocent));
    }

    private string[] LeftoverRewrites() =>
        Directory.GetFiles(_root, "*" + ArchiveEditExecutor.RewriteMarker + "*");
}
