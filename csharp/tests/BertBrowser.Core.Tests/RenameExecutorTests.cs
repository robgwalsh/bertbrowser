using BertBrowser.Core.Services.Rename;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Renames real files and folders on disk. Every test asserts on file <em>contents</em>, so a
/// rename that overwrites or loses data fails loudly rather than merely producing the right names.
/// </summary>
public sealed class RenameExecutorTests : IDisposable
{
    private readonly string _root;
    private readonly RenamePlanner _planner = new();
    private readonly RenameExecutor _executor = new();

    public RenameExecutorTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-rename-{Guid.NewGuid():N}");
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

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private string File_(string content, params string[] parts)
    {
        var path = P(parts);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    private string Dir(params string[] parts)
    {
        var path = P(parts);
        Directory.CreateDirectory(path);
        return path;
    }

    private RenameOutcome Run(IReadOnlyList<RenameSource> sources, string pattern) =>
        _executor.Execute(_planner.Plan(sources, pattern));

    private static RenameSource F(string path) => new(path, IsDirectory: false);

    private static RenameSource D(string path) => new(path, IsDirectory: true);

    private static void AssertContent(string path, string expected)
    {
        Assert.True(File.Exists(path), $"expected a file at {path}");
        Assert.Equal(expected, File.ReadAllText(path));
    }

    /// <summary>The temporary names staging uses must never outlive the rename.</summary>
    private void AssertNothingStranded() =>
        Assert.Empty(Directory.EnumerateFileSystemEntries(_root, ".bertbrowser-rename-*"));

    // --- the ordinary cases ---

    [Fact]
    public void OneFile_IsRenamedInPlace()
    {
        var source = File_("hello", "a.txt");

        var outcome = Run([F(source)], "b.txt");

        Assert.Empty(outcome.Failed);
        Assert.Single(outcome.Completed);
        AssertContent(P("b.txt"), "hello");
        Assert.False(File.Exists(source));
    }

    [Fact]
    public void OneFolder_IsRenamedWithItsContents()
    {
        var source = Dir("stuff");
        File_("inner", "stuff", "deep", "file.txt");

        var outcome = Run([D(source)], "things");

        Assert.Empty(outcome.Failed);
        AssertContent(P("things", "deep", "file.txt"), "inner");
        Assert.False(Directory.Exists(source));
    }

    [Fact]
    public void SeveralFiles_AreNumberedAndKeepTheirContents()
    {
        var a = File_("one", "a.jpg");
        var b = File_("two", "b.png");

        var outcome = Run([F(a), F(b)], "Trip");

        Assert.Empty(outcome.Failed);
        AssertContent(P("Trip 1.jpg"), "one");
        AssertContent(P("Trip 2.png"), "two");
        AssertNothingStranded();
    }

    [Fact]
    public void ChangingOnlyTheCasing_Works_ForAFile()
    {
        var source = File_("hello", "readme.md");

        var outcome = Run([F(source)], "README.md");

        Assert.Empty(outcome.Failed);
        AssertContent(P("README.md"), "hello");
        Assert.Equal("README.md", Path.GetFileName(Directory.GetFiles(_root).Single()));
        AssertNothingStranded();
    }

    [Fact]
    public void ChangingOnlyTheCasing_Works_ForAFolder()
    {
        var source = Dir("stuff");
        File_("inner", "stuff", "file.txt");

        var outcome = Run([D(source)], "STUFF");

        Assert.Empty(outcome.Failed);
        Assert.Equal("STUFF", Path.GetFileName(Directory.GetDirectories(_root).Single()));
        AssertContent(P("STUFF", "file.txt"), "inner");
        AssertNothingStranded();
    }

    // --- names that overlap inside the batch ---

    [Fact]
    public void RenamingASetOntoItsOwnNames_RotatesWithoutLosingAnything()
    {
        // Every target is a name the batch already holds: "Trip 2" -> "Trip 1" and back.
        var first = File_("one", "Trip 1.jpg");
        var second = File_("two", "Trip 2.jpg");

        var outcome = Run([F(second), F(first)], "Trip");

        Assert.Empty(outcome.Failed);
        Assert.Equal(2, outcome.Completed.Count);
        AssertContent(P("Trip 1.jpg"), "two");
        AssertContent(P("Trip 2.jpg"), "one");
        AssertNothingStranded();
    }

    [Fact]
    public void ShiftingNamesAlong_KeepsEveryFile()
    {
        // a -> "Note 1", and the file already called "Note 1" moves on to "Note 2".
        var a = File_("alpha", "a.txt");
        var note1 = File_("beta", "Note 1.txt");

        var outcome = Run([F(a), F(note1)], "Note");

        Assert.Empty(outcome.Failed);
        AssertContent(P("Note 1.txt"), "alpha");
        AssertContent(P("Note 2.txt"), "beta");
        AssertNothingStranded();
    }

    [Fact]
    public void RenamingFoldersOntoEachOthersNames_KeepsBothTrees()
    {
        var a = Dir("Work 1");
        var b = Dir("Work 2");
        File_("in-a", "Work 1", "a.txt");
        File_("in-b", "Work 2", "b.txt");

        var outcome = Run([D(b), D(a)], "Work");

        Assert.Empty(outcome.Failed);
        AssertContent(P("Work 1", "b.txt"), "in-b");
        AssertContent(P("Work 2", "a.txt"), "in-a");
        AssertNothingStranded();
    }

    // --- nothing is ever overwritten ---

    [Fact]
    public void ANameTakenBySomethingUnselected_IsRefusedAndLeavesBothFilesAlone()
    {
        var source = File_("mine", "a.txt");
        File_("theirs", "taken.txt");

        var outcome = Run([F(source)], "taken.txt");

        Assert.Empty(outcome.Completed);
        AssertContent(P("taken.txt"), "theirs"); // untouched
        AssertContent(source, "mine");
    }

    [Fact]
    public void ANameTakenAfterThePlanWasMade_FailsThatItemWithoutOverwriting()
    {
        // The plan is built while the dialog is open; disk can change under it, so the executor
        // checks again rather than trusting what the planner saw.
        var source = File_("mine", "a.txt");
        var plan = _planner.Plan([F(source)], "b.txt");
        File_("theirs", "b.txt"); // appears after planning

        var outcome = _executor.Execute(plan);

        Assert.Empty(outcome.Completed);
        Assert.Single(outcome.Failed);
        AssertContent(P("b.txt"), "theirs");
        AssertContent(source, "mine");
    }

    [Fact]
    public void AMissingItem_FailsWithoutStoppingTheRest()
    {
        var a = File_("one", "a.txt");
        var b = File_("two", "b.txt");
        var plan = _planner.Plan([F(a), F(b)], "Note");
        File.Delete(a); // disappears after planning

        var outcome = _executor.Execute(plan);

        Assert.Single(outcome.Failed);
        AssertContent(P("Note 2.txt"), "two");
        AssertNothingStranded();
    }

    [Fact]
    public void ANoOpRename_WritesNothing()
    {
        var source = File_("hello", "a.txt");

        var outcome = Run([F(source)], "a.txt");

        Assert.Empty(outcome.Completed);
        Assert.Empty(outcome.Failed);
        AssertContent(source, "hello");
    }

    // --- undo ---

    [Fact]
    public void Undo_PutsEveryNameBack()
    {
        var a = File_("one", "a.jpg");
        var b = File_("two", "b.png");
        var outcome = Run([F(a), F(b)], "Trip");

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(a, "one");
        AssertContent(b, "two");
        Assert.False(File.Exists(P("Trip 1.jpg")));
        AssertNothingStranded();
    }

    [Fact]
    public void Undo_ReversesARotation()
    {
        // The contents swapped places; undoing has to swap them back, staging and all.
        var first = File_("one", "Trip 1.jpg");
        var second = File_("two", "Trip 2.jpg");
        var outcome = Run([F(second), F(first)], "Trip");
        AssertContent(P("Trip 1.jpg"), "two");

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(P("Trip 1.jpg"), "one");
        AssertContent(P("Trip 2.jpg"), "two");
        AssertNothingStranded();
    }

    [Fact]
    public void Undo_PutsAFolderBackWithItsContents()
    {
        var source = Dir("stuff");
        File_("inner", "stuff", "deep", "file.txt");
        var outcome = Run([D(source)], "things");

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        AssertContent(P("stuff", "deep", "file.txt"), "inner");
        Assert.False(Directory.Exists(P("things")));
    }

    [Fact]
    public void Undo_RefusesToOverwriteSomethingThatTookTheOldName()
    {
        var source = File_("mine", "a.txt");
        var outcome = Run([F(source)], "b.txt");
        File_("someone else's", "a.txt"); // the old name is taken again

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Completed);
        Assert.Single(undo.Failed);
        AssertContent(P("a.txt"), "someone else's");
        AssertContent(P("b.txt"), "mine");
    }

    [Fact]
    public void Undo_ReportsAnItemThatIsNoLongerThere()
    {
        var source = File_("mine", "a.txt");
        var outcome = Run([F(source)], "b.txt");
        File.Delete(P("b.txt"));

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Completed);
        Assert.Single(undo.Failed);
    }

    [Fact]
    public void Undo_OfANoOpRename_HasNothingToDo() =>
        Assert.False(RenameOutcome.Empty.CanUndo);

    // --- meta-test: the stranding check can actually fail ---

    [Fact]
    public void TheStagingCheckNoticesALeftoverTemporaryName()
    {
        File_("x", ".bertbrowser-rename-deadbeef");

        Assert.ThrowsAny<Xunit.Sdk.XunitException>(AssertNothingStranded);
    }
}
