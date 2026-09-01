using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The one resolution a copy may use to take over an existing name, and the undo that pays for it.
/// Every test asserts on file <em>contents</em>, following <see cref="TransferExecutorTests"/>:
/// a replacement that loses the original is exactly the failure that would otherwise pass.
/// </summary>
public sealed class SyncOverwriteTests : IDisposable
{
    private readonly string _root;
    private readonly TransferPlanner _planner = new();
    private readonly TransferExecutor _executor = new();

    public SyncOverwriteTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-ovw-{Guid.NewGuid():N}");
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
        }
    }

    // --- helpers ---

    private string Dir(params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(path);
        return path;
    }

    private string File_(string content, params string[] parts)
    {
        var path = Path.Combine([_root, .. parts]);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private TransferOutcome Run(
        string[] sources, string destination, ConflictResolution resolution,
        TransferVerb verb = TransferVerb.Copy)
    {
        var plan = _planner.Plan(sources, destination, verb);
        var resolutions = plan.Transfers.ToDictionary(
            t => PathKey.Canonicalize(t.SourcePath), _ => resolution);
        return _executor.Execute(plan, resolutions);
    }

    private static void AssertContent(string path, string expected)
    {
        Assert.True(File.Exists(path), $"expected a file at {path}");
        Assert.Equal(expected, File.ReadAllText(path));
    }

    private static IReadOnlyList<string> Staged(TransferOutcome outcome) =>
        TransferExecutor.StagedItems(outcome);

    // --- writing ---

    [Fact]
    public void ACopyWithOverwriteTakesTheName()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite);

        AssertContent(P("dst", "a.txt"), "new");
    }

    /// <summary>The whole reason the value is allowed to exist: what it displaced is set aside
    /// intact, not erased to make room.</summary>
    [Fact]
    public void TheDisplacedFileIsIntactInStaging()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite);

        AssertContent(Assert.Single(Staged(outcome)), "old");
    }

    /// <summary>The existing rule, left exactly as it was. A drag-drop copy offering Replace must
    /// still land beside the original rather than over it.</summary>
    [Fact]
    public void ReplaceOnACopyIsStillDowngradedToKeepBoth()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Replace);

        AssertContent(P("dst", "a.txt"), "old");
        AssertContent(P("dst", "a (2).txt"), "new");
        Assert.Empty(Staged(outcome));
    }

    /// <summary>An ordinary copy stays out of the shared undo slot. Only a caller holding a sync
    /// outcome may reverse one, and it comes in through <see cref="TransferExecutor.UndoCopies"/>.</summary>
    [Fact]
    public void ACopyOutcomeIsStillNotUndoableOnItsOwn()
    {
        File_("new", "src", "a.txt");

        Assert.False(Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite).CanUndo);
    }

    [Fact]
    public void AFolderCanBeOverwrittenToo()
    {
        File_("new", "src", "lib", "x.txt");
        File_("old", "dst", "lib", "x.txt");
        File_("only-old", "dst", "lib", "gone.txt");

        var outcome = Run([P("src", "lib")], Dir("dst"), ConflictResolution.Overwrite);

        AssertContent(P("dst", "lib", "x.txt"), "new");
        Assert.False(File.Exists(P("dst", "lib", "gone.txt")));
        AssertContent(Path.Combine(Assert.Single(Staged(outcome)), "gone.txt"), "only-old");
    }

    // --- undo ---

    [Fact]
    public void UndoPutsTheDisplacedFileBackAndRemovesTheCopy()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite);
        var undo = _executor.UndoCopies(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(P("dst", "a.txt"), "old");
        AssertContent(P("src", "a.txt"), "new"); // the source was never touched
    }

    /// <summary>
    /// A sync adds as well as replaces, and the undo the user was offered covers both. A file the
    /// run put at a name that was free has to go, or the right side is left half-synced with no way
    /// to tell which half.
    /// </summary>
    [Fact]
    public void UndoRemovesWhatTheCopyMerelyAdded()
    {
        File_("new", "src", "fresh.txt");

        var outcome = Run([P("src", "fresh.txt")], Dir("dst"), ConflictResolution.Overwrite);
        AssertContent(P("dst", "fresh.txt"), "new");

        _executor.UndoCopies(outcome);

        Assert.False(File.Exists(P("dst", "fresh.txt")));
        AssertContent(P("src", "fresh.txt"), "new");
    }

    [Fact]
    public void UndoRemovesAWholeCopiedFolder()
    {
        File_("a", "src", "lib", "deep", "x.txt");

        var outcome = Run([P("src", "lib")], Dir("dst"), ConflictResolution.Overwrite);
        Assert.True(Directory.Exists(P("dst", "lib", "deep")));

        _executor.UndoCopies(outcome);

        Assert.False(Directory.Exists(P("dst", "lib")));
        AssertContent(P("src", "lib", "deep", "x.txt"), "a");
    }

    /// <summary>Undo leaves no staging folder behind once it has emptied it — otherwise every
    /// reversed sync would leave a hidden directory in the destination for good.</summary>
    [Fact]
    public void UndoClearsTheStagingFolderItEmptied()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite);
        _executor.UndoCopies(outcome);

        Assert.Empty(Staged(outcome));
        Assert.False(Directory.Exists(Assert.Single(outcome.StagingDirectories)));
    }

    [Fact]
    public void UndoRefusesAMove()
    {
        File_("x", "src", "a.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.KeepBoth, TransferVerb.Move);

        Assert.NotEmpty(_executor.UndoCopies(outcome).Failed);
        AssertContent(P("dst", "a.txt"), "x");
    }

    /// <summary>Once the run can no longer be undone the displaced original is finally gone, and
    /// nothing else with it.</summary>
    [Fact]
    public void CommitStagingErasesTheDisplacedOriginalAndNothingElse()
    {
        File_("new", "src", "a.txt");
        File_("old", "dst", "a.txt");
        File_("bystander", "dst", "b.txt");

        var outcome = Run([P("src", "a.txt")], Dir("dst"), ConflictResolution.Overwrite);
        TransferExecutor.CommitStaging(outcome);

        Assert.Empty(Staged(outcome));
        AssertContent(P("dst", "a.txt"), "new");
        AssertContent(P("dst", "b.txt"), "bystander");
        AssertContent(P("src", "a.txt"), "new");
    }
}
