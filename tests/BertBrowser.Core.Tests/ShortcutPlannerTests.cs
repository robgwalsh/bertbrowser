using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Shortcuts;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What "Create shortcuts here" would produce. Run against a fake filesystem: the interesting cases
/// are collisions — with what is already in the folder, and with each other — and setting those up
/// for real would mean writing <c>.lnk</c> files to find out.
/// </summary>
public sealed class ShortcutPlannerTests
{
    private readonly FakeShortcutProbe _probe = new();
    private readonly ShortcutPlanner _planner;

    public ShortcutPlannerTests()
    {
        _planner = new ShortcutPlanner(_probe);
        _probe.AddDirectory(@"C:\dest");
    }

    private ShortcutPlan Plan(params string[] sources) => _planner.Plan(sources, @"C:\dest");

    private static string LinkFor(ShortcutPlan plan, string source) =>
        plan.Creations.Single(c => c.TargetPath.Equals(source, StringComparison.OrdinalIgnoreCase))
            .LinkPath;

    // --- Naming: Explorer's rule, extension and all ---

    [Fact]
    public void AFileGetsANameAndShortcutLink()
    {
        _probe.AddFile(@"C:\src\notes.txt");

        var plan = Plan(@"C:\src\notes.txt");

        Assert.Empty(plan.Problems);
        Assert.Equal(@"C:\dest\notes.txt - Shortcut.lnk", Assert.Single(plan.Creations).LinkPath);
    }

    /// <summary>A folder keeps its whole name too — the number, when one is needed, goes before the
    /// <c>.lnk</c> because the thing being created is a file whatever it points at.</summary>
    [Fact]
    public void AFolderGetsTheSameTreatment()
    {
        _probe.AddDirectory(@"C:\src\Reports");

        var plan = Plan(@"C:\src\Reports");

        Assert.Equal(@"C:\dest\Reports - Shortcut.lnk", Assert.Single(plan.Creations).LinkPath);
    }

    [Fact]
    public void ATakenNameStepsAside()
    {
        _probe.AddFile(@"C:\src\notes.txt");
        _probe.AddFile(@"C:\dest\notes.txt - Shortcut.lnk");

        var plan = Plan(@"C:\src\notes.txt");

        Assert.Equal(@"C:\dest\notes.txt - Shortcut (2).lnk", Assert.Single(plan.Creations).LinkPath);
    }

    /// <summary>
    /// The one a per-item rule gets wrong. Two files with the same name from different folders are
    /// one drag, and nothing is on disk yet — so a planner that only asks the probe hands both the
    /// same link path and the second write silently replaces the first.
    /// </summary>
    [Fact]
    public void TwoSourcesNamedAlikeGetDifferentLinks()
    {
        _probe.AddFile(@"C:\one\notes.txt");
        _probe.AddFile(@"C:\two\notes.txt");

        var plan = Plan(@"C:\one\notes.txt", @"C:\two\notes.txt");

        Assert.Equal(@"C:\dest\notes.txt - Shortcut.lnk", LinkFor(plan, @"C:\one\notes.txt"));
        Assert.Equal(@"C:\dest\notes.txt - Shortcut (2).lnk", LinkFor(plan, @"C:\two\notes.txt"));
    }

    // --- Refusals ---

    /// <summary>One item's problem never costs the others theirs — the same rule every other
    /// planner in the app follows.</summary>
    [Fact]
    public void AMissingSourceIsRefused_AndTheRestArePlanned()
    {
        _probe.AddFile(@"C:\src\here.txt");

        var plan = Plan(@"C:\src\gone.txt", @"C:\src\here.txt");

        Assert.Equal(@"C:\src\gone.txt", Assert.Single(plan.Problems).SourcePath);
        Assert.Equal(@"C:\dest\here.txt - Shortcut.lnk", Assert.Single(plan.Creations).LinkPath);
    }

    /// <summary>
    /// An item inside an archive has a path that reads like a real one but names nothing on disk, so
    /// there is nothing for a <c>.lnk</c> to point at. No archive knowledge is needed to refuse it:
    /// it simply does not exist, which is the rule that was already there.
    /// </summary>
    [Fact]
    public void AnItemInsideAnArchiveIsRefused()
    {
        _probe.AddFile(@"C:\src\bundle.zip");

        var plan = Plan(@"C:\src\bundle.zip\inner\notes.txt");

        Assert.Empty(plan.Creations);
        Assert.Single(plan.Problems);
    }

    [Fact]
    public void AMissingDestinationRefusesEverything()
    {
        _probe.AddFile(@"C:\src\notes.txt");

        var plan = _planner.Plan([@"C:\src\notes.txt"], @"C:\nowhere");

        Assert.Empty(plan.Creations);
        Assert.False(plan.HasWork);
        Assert.Single(plan.Problems);
    }

    [Fact]
    public void APlanWithNothingInItHasNoWork()
    {
        Assert.False(_planner.Plan([], @"C:\dest").HasWork);
    }
}

internal sealed class FakeShortcutProbe : IShortcutProbe
{
    private readonly HashSet<string> _directories = new(StringComparer.Ordinal);
    private readonly HashSet<string> _files = new(StringComparer.Ordinal);

    public void AddDirectory(string path) => _directories.Add(PathKey.Canonicalize(path));

    public void AddFile(string path) => _files.Add(PathKey.Canonicalize(path));

    public bool DirectoryExists(string path) => _directories.Contains(PathKey.Canonicalize(path));

    public bool FileExists(string path) => _files.Contains(PathKey.Canonicalize(path));
}
