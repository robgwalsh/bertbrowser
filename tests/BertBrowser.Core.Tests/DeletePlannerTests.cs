using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The rules that decide what a delete is allowed to reach. Run against a fake filesystem, because
/// the interesting cases — a drive root, the Windows folder, a folder selected together with
/// something inside it — are ones nobody should set up for real to find out.
/// </summary>
public sealed class DeletePlannerTests
{
    private readonly FakeDeleteProbe _probe = new();
    private readonly DeletePlanner _planner;

    public DeletePlannerTests() =>
        _planner = new DeletePlanner(_probe, [@"C:\Windows", @"C:\Users\rob"]);

    private static DeleteSource File_(string path) => new(path, IsDirectory: false);

    private static DeleteSource Dir(string path) => new(path, IsDirectory: true);

    private static DeleteRejection ReasonFor(DeletePlan plan, string source) =>
        plan.Rejected.Single(r => r.SourcePath.Equals(source, StringComparison.OrdinalIgnoreCase)).Reason;

    private DeletePlan Plan(params DeleteSource[] sources) => _planner.Plan(sources, permanent: false);

    // --- the ordinary cases ---

    [Fact]
    public void AFile_IsPlanned()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = Plan(File_(@"C:\p\a.txt"));

        Assert.Empty(plan.Rejected);
        var deletion = Assert.Single(plan.Deletions);
        Assert.Equal(@"C:\p\a.txt", deletion.SourcePath);
        Assert.False(deletion.IsDirectory);
    }

    [Fact]
    public void AFolder_IsPlannedAsADirectory_WhateverTheCallerClaimed()
    {
        _probe.AddDirectory(@"C:\p\sub");

        // The selection says "file"; disk says otherwise, and disk wins — the executor needs the
        // right answer to know whether to move a file or a directory.
        var plan = Plan(File_(@"C:\p\sub"));

        Assert.True(Assert.Single(plan.Deletions).IsDirectory);
    }

    [Fact]
    public void TheSameItemTwice_IsPlannedOnce()
    {
        _probe.AddFile(@"C:\p\a.txt");

        var plan = Plan(File_(@"C:\p\a.txt"), File_(@"C:\P\A.TXT"));

        Assert.Single(plan.Deletions);
    }

    [Fact]
    public void NoSources_IsAnEmptyPlan()
    {
        var plan = _planner.Plan([], permanent: true);

        Assert.False(plan.HasWork);
        Assert.True(plan.Permanent);
    }

    // --- the refusals ---

    [Fact]
    public void AnItemThatHasGone_IsRefused()
    {
        var plan = Plan(File_(@"C:\p\vanished.txt"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.SourceMissing, ReasonFor(plan, @"C:\p\vanished.txt"));
    }

    [Fact]
    public void ADriveRoot_IsRefused()
    {
        _probe.AddDirectory(@"D:\");

        var plan = Plan(Dir(@"D:\"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.SourceIsRoot, ReasonFor(plan, @"D:\"));
    }

    [Fact]
    public void AProtectedLocation_IsRefused()
    {
        _probe.AddDirectory(@"C:\Windows");

        var plan = Plan(Dir(@"C:\Windows"));

        Assert.False(plan.HasWork);
        Assert.Equal(DeleteRejection.ProtectedLocation, ReasonFor(plan, @"C:\Windows"));
    }

    [Fact]
    public void SomethingInsideAProtectedLocation_IsStillDeletable()
    {
        // The folder is protected, not everything under it — this is a file browser.
        _probe.AddFile(@"C:\Windows\Temp\junk.log");

        var plan = Plan(File_(@"C:\Windows\Temp\junk.log"));

        Assert.Single(plan.Deletions);
    }

    [Fact]
    public void AnItemInsideAFolderBeingDeleted_TravelsWithItAndIsNotReported()
    {
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddFile(@"C:\p\sub\a.txt");

        var plan = Plan(Dir(@"C:\p\sub"), File_(@"C:\p\sub\a.txt"));

        Assert.Equal(@"C:\p\sub", Assert.Single(plan.Deletions).SourcePath);
        Assert.Equal(DeleteRejection.InsideADeletedFolder, ReasonFor(plan, @"C:\p\sub\a.txt"));
        Assert.Empty(plan.Problems); // benign: it is being deleted, just not separately
    }

    [Fact]
    public void ADeeplyNestedItem_TravelsWithItsAncestorToo()
    {
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddDirectory(@"C:\p\sub\deep\deeper");

        var plan = Plan(Dir(@"C:\p\sub"), Dir(@"C:\p\sub\deep\deeper"));

        Assert.Equal(@"C:\p\sub", Assert.Single(plan.Deletions).SourcePath);
    }

    [Fact]
    public void ASiblingWithASharedNamePrefix_IsNotMistakenForAChild()
    {
        // "C:\p\sub2" starts with "C:\p\sub" as a string but is not inside it.
        _probe.AddDirectory(@"C:\p\sub");
        _probe.AddDirectory(@"C:\p\sub2");

        var plan = Plan(Dir(@"C:\p\sub"), Dir(@"C:\p\sub2"));

        Assert.Equal(2, plan.Deletions.Count);
        Assert.Empty(plan.Rejected);
    }

    [Fact]
    public void OneRefusalDoesNotStopTheRest()
    {
        _probe.AddFile(@"C:\p\a.txt");
        _probe.AddDirectory(@"C:\Windows");

        var plan = Plan(Dir(@"C:\Windows"), File_(@"C:\p\a.txt"));

        Assert.Equal(@"C:\p\a.txt", Assert.Single(plan.Deletions).SourcePath);
        Assert.Single(plan.Problems);
    }
}

internal sealed class FakeDeleteProbe : IDeleteProbe
{
    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly HashSet<string> _files = new(StringComparer.Ordinal);

    public void AddDirectory(string path) => _directories.Add(PathKey.Canonicalize(path));

    public void AddFile(string path) => _files.Add(PathKey.Canonicalize(path));

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.Contains(PathKey.Canonicalize(path));
}
