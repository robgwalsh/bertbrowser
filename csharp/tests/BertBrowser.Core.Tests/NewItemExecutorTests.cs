using BertBrowser.Core.Services.NewItem;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Creates real folders and files on a real disk. The failure worth catching here is a silent one:
/// <c>Directory.CreateDirectory</c> succeeds on a folder that is already there, so a create that
/// merely reported success could hand the user somebody else's folder. Every test asserts on what
/// is on disk afterwards rather than on the outcome record alone.
/// </summary>
public sealed class NewItemExecutorTests : IDisposable
{
    private readonly string _root;
    private readonly NewItemPlanner _planner = new();
    private readonly NewItemExecutor _executor = new();

    public NewItemExecutorTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-newitem-{Guid.NewGuid():N}");
        Directory.CreateDirectory(_root);
    }

    public void Dispose()
    {
        // A test that made something read-only would otherwise strand the temp tree.
        foreach (var path in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
        {
            try { File.SetAttributes(path, FileAttributes.Normal); }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { }
        }

        try { Directory.Delete(_root, recursive: true); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { }
    }

    private string P(params string[] parts) => Path.Combine([_root, .. parts]);

    private void File_(string content, params string[] parts)
    {
        var path = P(parts);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
    }

    private NewItemOutcome Run(string name, NewItemKind kind, string? template = null) =>
        _executor.Execute(_planner.Plan(_root, name, kind, template));

    private static void AssertContent(string path, string expected) =>
        Assert.Equal(expected, File.ReadAllText(path));

    /// <summary>The root holds exactly these names and nothing else.</summary>
    private void AssertRootHolds(params string[] names) =>
        Assert.Equal(
            names.OrderBy(n => n, StringComparer.Ordinal).ToList(),
            Directory.EnumerateFileSystemEntries(_root)
                .Select(p => Path.GetFileName(p)!)
                .OrderBy(n => n, StringComparer.Ordinal)
                .ToList());

    // --- the ordinary cases ---

    [Fact]
    public void AFolderIsCreated()
    {
        var outcome = Run("Reports", NewItemKind.Folder);
        Assert.Equal(P("Reports"), outcome.CreatedPath);
        Assert.True(Directory.Exists(P("Reports")));
        AssertRootHolds("Reports");
    }

    [Fact]
    public void ANewFileIsCreatedAndIsActuallyEmpty()
    {
        var outcome = Run("notes.txt", NewItemKind.File);
        Assert.Equal(P("notes.txt"), outcome.CreatedPath);
        AssertContent(P("notes.txt"), "");
        // Created and closed — nothing may still hold a handle on it.
        File.AppendAllText(P("notes.txt"), "x");
    }

    [Fact]
    public void ATemplateIsCopiedContentsAndAll()
    {
        File_("Dear {name},", "templates", "letter.rtf");
        var outcome = Run("letter.rtf", NewItemKind.File, P("templates", "letter.rtf"));
        Assert.NotNull(outcome.CreatedPath);
        AssertContent(P("letter.rtf"), "Dear {name},");
    }

    [Fact]
    public void ACopiedTemplateDoesNotInheritBeingHiddenOrReadOnly()
    {
        // The shipped templates live under %APPDATA%\Microsoft\Windows\Templates and are commonly
        // both; the file the user asked for should be neither.
        File_("x", "templates", "letter.rtf");
        var template = P("templates", "letter.rtf");
        File.SetAttributes(template, FileAttributes.Hidden | FileAttributes.ReadOnly);

        Run("letter.rtf", NewItemKind.File, template);

        var attributes = File.GetAttributes(P("letter.rtf"));
        Assert.False(attributes.HasFlag(FileAttributes.Hidden));
        Assert.False(attributes.HasFlag(FileAttributes.ReadOnly));
    }

    // --- nothing is overwritten, and nothing existing is adopted ---

    [Fact]
    public void AFolderThatAppearedSinceThePlan_FailsRatherThanBeingAdopted()
    {
        // The plan is built while the dialog sits open, so it is advisory. Drop the executor's
        // existence check and this goes green with the user's folder silently handed back.
        var plan = _planner.Plan(_root, "Reports", NewItemKind.Folder);
        Directory.CreateDirectory(P("Reports"));
        File_("theirs", "Reports", "already-here.txt");

        var outcome = _executor.Execute(plan);

        Assert.Null(outcome.CreatedPath);
        Assert.NotNull(outcome.Failed);
        AssertContent(P("Reports", "already-here.txt"), "theirs");
    }

    [Fact]
    public void AFileThatAppearedSinceThePlan_KeepsItsContents()
    {
        var plan = _planner.Plan(_root, "notes.txt", NewItemKind.File);
        File_("theirs", "notes.txt");

        var outcome = _executor.Execute(plan);

        Assert.Null(outcome.CreatedPath);
        Assert.NotNull(outcome.Failed);
        AssertContent(P("notes.txt"), "theirs");
    }

    [Fact]
    public void ATemplateThatWentMissingSinceThePlan_FailsAndWritesNothing()
    {
        File_("x", "templates", "letter.rtf");
        var template = P("templates", "letter.rtf");
        var plan = _planner.Plan(_root, "letter.rtf", NewItemKind.File, template);
        File.Delete(template);

        var outcome = _executor.Execute(plan);

        Assert.NotNull(outcome.Failed);
        Assert.False(File.Exists(P("letter.rtf")));
    }

    [Fact]
    public void ARefusedPlanLeavesTheFolderExactlyAsItWas()
    {
        File_("mine", "notes.txt");
        var plan = _planner.Plan(_root, "notes.txt", NewItemKind.File);
        Assert.False(plan.HasWork);

        _executor.Execute(plan);

        AssertRootHolds("notes.txt");
        AssertContent(P("notes.txt"), "mine");
    }

    // --- meta-test: the "exactly these names" check can actually fail ---

    [Fact]
    public void TheRootListingCheckNoticesAnExtraEntry()
    {
        File_("x", "notes.txt");
        File_("y", "unexpected.txt");
        Assert.ThrowsAny<Xunit.Sdk.XunitException>(() => AssertRootHolds("notes.txt"));
    }
}
