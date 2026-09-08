using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Shortcuts;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Writing the links a <see cref="ShortcutPlan"/> asked for. The writer is faked because what is
/// worth testing here is not the <c>.lnk</c> format — that is the shell's job — but the two rules
/// every executor in this app obeys: re-check live disk before writing, and never let one item's
/// failure cost the others theirs.
/// </summary>
public sealed class ShortcutExecutorTests
{
    private readonly FakeShortcutProbe _probe = new();
    private readonly FakeShortcutWriter _writer = new();
    private readonly ShortcutExecutor _executor;

    public ShortcutExecutorTests()
    {
        _executor = new ShortcutExecutor(_writer, _probe);
        _probe.AddDirectory(@"C:\dest");
    }

    private static ShortcutPlan PlanOf(params ShortcutCreation[] creations) =>
        new(@"C:\dest", creations, []);

    [Fact]
    public void WritesOneLinkPerCreation()
    {
        var outcome = _executor.Execute(PlanOf(
            new ShortcutCreation(@"C:\src\a.txt", @"C:\dest\a.txt - Shortcut.lnk"),
            new ShortcutCreation(@"C:\src\b.txt", @"C:\dest\b.txt - Shortcut.lnk")));

        Assert.Empty(outcome.Failed);
        Assert.Equal(2, outcome.Created.Count);
        Assert.Equal(@"C:\src\a.txt", _writer.TargetOf(@"C:\dest\a.txt - Shortcut.lnk"));
        Assert.Equal(@"C:\src\b.txt", _writer.TargetOf(@"C:\dest\b.txt - Shortcut.lnk"));
    }

    /// <summary>
    /// The plan was made while the menu was open; anything could have appeared since. Writing over
    /// it would destroy a file the user never named — so the "(2)" rule runs again against live
    /// disk, exactly as the transfer and new-item executors re-check theirs.
    /// </summary>
    [Fact]
    public void ANameTakenSincePlanningStepsAside_RatherThanOverwriting()
    {
        _probe.AddFile(@"C:\dest\a.txt - Shortcut.lnk");

        var outcome = _executor.Execute(PlanOf(
            new ShortcutCreation(@"C:\src\a.txt", @"C:\dest\a.txt - Shortcut.lnk")));

        Assert.Equal(@"C:\dest\a.txt - Shortcut (2).lnk", Assert.Single(outcome.Created));
        Assert.False(_writer.Wrote(@"C:\dest\a.txt - Shortcut.lnk"));
    }

    [Fact]
    public void OneFailureDoesNotStopTheOthers()
    {
        _writer.FailOn(@"C:\dest\a.txt - Shortcut.lnk", new IOException("the disk said no"));

        var outcome = _executor.Execute(PlanOf(
            new ShortcutCreation(@"C:\src\a.txt", @"C:\dest\a.txt - Shortcut.lnk"),
            new ShortcutCreation(@"C:\src\b.txt", @"C:\dest\b.txt - Shortcut.lnk")));

        Assert.Equal(@"C:\dest\b.txt - Shortcut.lnk", Assert.Single(outcome.Created));
        Assert.Equal(@"C:\dest\a.txt - Shortcut.lnk", Assert.Single(outcome.Failed).LinkPath);
    }

    /// <summary>The one failure an administrator token could fix, flagged so the message can say
    /// so rather than blaming the path.</summary>
    [Fact]
    public void ARefusedFolderIsReportedAsAccessDenied()
    {
        _writer.FailOn(@"C:\dest\a.txt - Shortcut.lnk", new UnauthorizedAccessException("no"));

        var outcome = _executor.Execute(PlanOf(
            new ShortcutCreation(@"C:\src\a.txt", @"C:\dest\a.txt - Shortcut.lnk")));

        Assert.True(Assert.Single(outcome.Failed).AccessDenied);
    }

    [Fact]
    public void AnEmptyPlanWritesNothing()
    {
        var outcome = _executor.Execute(ShortcutPlan.Empty);

        Assert.Empty(outcome.Created);
        Assert.Empty(outcome.Failed);
        Assert.Empty(_writer.Written);
    }
}

internal sealed class FakeShortcutWriter : IShortcutWriter
{
    private readonly Dictionary<string, Exception> _failures = new(StringComparer.Ordinal);

    public Dictionary<string, string> Written { get; } = new(StringComparer.Ordinal);

    public void FailOn(string linkPath, Exception failure) =>
        _failures[PathKey.Canonicalize(linkPath)] = failure;

    public bool Wrote(string linkPath) => Written.ContainsKey(PathKey.Canonicalize(linkPath));

    public string TargetOf(string linkPath) => Written[PathKey.Canonicalize(linkPath)];

    public void Write(string linkPath, string targetPath)
    {
        var key = PathKey.Canonicalize(linkPath);
        if (_failures.TryGetValue(key, out var failure)) throw failure;
        Written[key] = targetPath;
    }
}
