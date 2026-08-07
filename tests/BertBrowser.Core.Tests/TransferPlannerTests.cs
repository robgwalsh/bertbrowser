using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Rules that decide whether a drop is allowed at all. These run against a fake filesystem so the
/// dangerous layouts — a folder dropped into its own subtree, reached directly or through a
/// junction — can be exercised without needing the privileges to create real links.
/// </summary>
public sealed class TransferPlannerTests
{
    private readonly FakeProbe _probe = new();
    private readonly TransferPlanner _planner;

    public TransferPlannerTests() => _planner = new TransferPlanner(_probe);

    private TransferPlan Plan(string[] sources, string destination, TransferVerb verb = TransferVerb.Move) =>
        _planner.Plan(sources, destination, verb);

    private static TransferRejection ReasonFor(TransferPlan plan, string source) =>
        plan.Rejected.Single(r => r.SourcePath.Equals(source, StringComparison.OrdinalIgnoreCase)).Reason;

    // --- The core case: a folder must never go inside itself ---

    [Fact]
    public void FolderDroppedOntoItself_IsRejected()
    {
        _probe.AddDirectory(@"C:\work\project");

        var plan = Plan([@"C:\work\project"], @"C:\work\project");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationIsSource, ReasonFor(plan, @"C:\work\project"));
    }

    [Fact]
    public void FolderDroppedOntoItself_IsRejected_RegardlessOfCasing()
    {
        _probe.AddDirectory(@"C:\work\project");

        var plan = Plan([@"C:\work\project"], @"C:\WORK\PROJECT");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationIsSource, ReasonFor(plan, @"C:\work\project"));
    }

    [Theory]
    [InlineData(@"C:\work\project\sub")]
    [InlineData(@"C:\work\project\sub\deeper\deepest")]
    public void FolderDroppedIntoItsOwnSubtree_IsRejected(string destination)
    {
        _probe.AddDirectory(@"C:\work\project");
        _probe.AddDirectory(destination);

        var plan = Plan([@"C:\work\project"], destination);

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationInsideSource, ReasonFor(plan, @"C:\work\project"));
    }

    [Fact]
    public void FolderDroppedIntoItsOwnSubtree_ThroughAJunction_IsRejected()
    {
        // C:\link is a junction to the folder being moved, so C:\link\sub is physically inside it
        // even though nothing about the literal paths says so.
        _probe.AddDirectory(@"C:\work\project");
        _probe.AddDirectory(@"C:\work\project\sub");
        _probe.AddLink(@"C:\link", @"C:\work\project");
        _probe.AddDirectory(@"C:\link\sub");

        var plan = Plan([@"C:\work\project"], @"C:\link\sub");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationInsideSource, ReasonFor(plan, @"C:\work\project"));
    }

    [Fact]
    public void FolderDroppedOntoAJunctionPointingAtItself_IsRejected()
    {
        _probe.AddDirectory(@"C:\work\project");
        _probe.AddLink(@"C:\link", @"C:\work\project");

        var plan = Plan([@"C:\work\project"], @"C:\link");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationInsideSource, ReasonFor(plan, @"C:\work\project"));
    }

    [Fact]
    public void FolderDroppedIntoAnUnrelatedJunction_IsAllowed()
    {
        _probe.AddDirectory(@"C:\work\project");
        _probe.AddDirectory(@"C:\elsewhere");
        _probe.AddLink(@"C:\link", @"C:\elsewhere");

        var plan = Plan([@"C:\work\project"], @"C:\link");

        Assert.Empty(plan.Problems);
        Assert.Equal(@"C:\link\project", plan.Transfers.Single().DestinationPath);
    }

    [Fact]
    public void SiblingWithASharedNamePrefix_IsNotTreatedAsASubtree()
    {
        // "C:\workspace" starts with "C:\work" as a string but is not inside it.
        _probe.AddDirectory(@"C:\work");
        _probe.AddDirectory(@"C:\workspace");

        var plan = Plan([@"C:\work"], @"C:\workspace");

        Assert.Empty(plan.Problems);
        Assert.Equal(@"C:\workspace\work", plan.Transfers.Single().DestinationPath);
    }

    [Fact]
    public void FileNamedLikeItsDestination_IsNotConfusedForAFolder()
    {
        // A file source can never contain the destination, so no containment rule applies.
        _probe.AddFile(@"C:\work\project");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\work\project"], @"C:\dest");

        Assert.Equal(@"C:\dest\project", plan.Transfers.Single().DestinationPath);
    }

    // --- Sources that travel with something else ---

    [Fact]
    public void SourceNestedUnderAnotherSource_IsDroppedFromThePlan()
    {
        _probe.AddDirectory(@"C:\src\tree");
        _probe.AddDirectory(@"C:\src\tree\inner");
        _probe.AddFile(@"C:\src\tree\inner\leaf.txt");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\src\tree", @"C:\src\tree\inner", @"C:\src\tree\inner\leaf.txt"], @"C:\dest");

        Assert.Equal(@"C:\src\tree", plan.Transfers.Single().SourcePath);
        Assert.Equal(TransferRejection.MovesWithAncestor, ReasonFor(plan, @"C:\src\tree\inner"));
        Assert.Equal(TransferRejection.MovesWithAncestor, ReasonFor(plan, @"C:\src\tree\inner\leaf.txt"));
        Assert.Empty(plan.Problems); // travelling with an ancestor is a no-op, not an error
    }

    [Fact]
    public void SiblingSourcesAreAllKept()
    {
        _probe.AddDirectory(@"C:\src\one");
        _probe.AddDirectory(@"C:\src\two");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\src\one", @"C:\src\two"], @"C:\dest");

        Assert.Equal(2, plan.Transfers.Count);
    }

    [Fact]
    public void RepeatedSources_AreDeduped_IncludingByCase()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\src\a.txt", @"C:\SRC\A.TXT", @"C:\src\a.txt"], @"C:\dest");

        Assert.Single(plan.Transfers);
    }

    // --- Roots, missing pieces, bad destinations ---

    [Theory]
    [InlineData(@"C:\")]
    [InlineData(@"D:\")]
    public void DriveRoot_IsRejected(string root)
    {
        _probe.AddDirectory(root);
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([root], @"C:\dest");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.SourceIsRoot, ReasonFor(plan, root));
    }

    [Fact]
    public void MissingSource_IsRejected_AndDoesNotStopTheOthers()
    {
        _probe.AddFile(@"C:\src\present.txt");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\src\gone.txt", @"C:\src\present.txt"], @"C:\dest");

        Assert.Equal(@"C:\src\present.txt", plan.Transfers.Single().SourcePath);
        Assert.Equal(TransferRejection.SourceMissing, ReasonFor(plan, @"C:\src\gone.txt"));
    }

    [Fact]
    public void MissingDestination_RejectsEverything()
    {
        _probe.AddFile(@"C:\src\a.txt");

        var plan = Plan([@"C:\src\a.txt"], @"C:\nope");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationMissing, ReasonFor(plan, @"C:\src\a.txt"));
    }

    [Fact]
    public void FileAsDestination_RejectsEverything()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddFile(@"C:\dest.txt");

        var plan = Plan([@"C:\src\a.txt"], @"C:\dest.txt");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.DestinationNotDirectory, ReasonFor(plan, @"C:\src\a.txt"));
    }

    [Fact]
    public void NoSources_ProducesNoWork()
    {
        _probe.AddDirectory(@"C:\dest");

        Assert.False(Plan([], @"C:\dest").HasWork);
    }

    // --- Same-folder drops ---

    [Fact]
    public void MoveIntoItsOwnFolder_IsANoOp()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddDirectory(@"C:\src");

        var plan = Plan([@"C:\src\a.txt"], @"C:\src");

        Assert.False(plan.HasWork);
        Assert.Equal(TransferRejection.AlreadyInDestination, ReasonFor(plan, @"C:\src\a.txt"));
        Assert.Empty(plan.Problems); // nothing to do is not an error
    }

    [Fact]
    public void CopyIntoItsOwnFolder_IsAllowedAndConflicts()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddDirectory(@"C:\src");

        var plan = Plan([@"C:\src\a.txt"], @"C:\src", TransferVerb.Copy);

        Assert.True(plan.Transfers.Single().Conflicts);
    }

    // --- Conflict detection ---

    [Fact]
    public void ExistingNameAtDestination_IsFlaggedAsAConflict()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddDirectory(@"C:\dest");
        _probe.AddFile(@"C:\dest\a.txt");

        var plan = Plan([@"C:\src\a.txt"], @"C:\dest");

        Assert.True(plan.Transfers.Single().Conflicts);
        Assert.Single(plan.Conflicts);
    }

    [Fact]
    public void ExistingFolderBlockingAFileName_IsAConflict()
    {
        _probe.AddFile(@"C:\src\report");
        _probe.AddDirectory(@"C:\dest");
        _probe.AddDirectory(@"C:\dest\report");

        var plan = Plan([@"C:\src\report"], @"C:\dest");

        Assert.True(plan.Transfers.Single().Conflicts);
    }

    [Fact]
    public void TwoSourcesWithTheSameName_SecondOneConflicts()
    {
        _probe.AddFile(@"C:\one\a.txt");
        _probe.AddFile(@"C:\two\a.txt");
        _probe.AddDirectory(@"C:\dest");

        var plan = Plan([@"C:\one\a.txt", @"C:\two\a.txt"], @"C:\dest");

        Assert.False(plan.Transfers[0].Conflicts);
        Assert.True(plan.Transfers[1].Conflicts);
    }

    [Fact]
    public void NoExistingName_IsNotAConflict()
    {
        _probe.AddFile(@"C:\src\a.txt");
        _probe.AddDirectory(@"C:\dest");

        Assert.False(Plan([@"C:\src\a.txt"], @"C:\dest").Transfers.Single().Conflicts);
    }

    /// <summary>In-memory filesystem: a set of directories and files, plus a link table used to
    /// resolve junction targets the same way the real probe walks them.</summary>
    private sealed class FakeProbe : ITransferProbe
    {
        private readonly HashSet<string> _directories = new(StringComparer.OrdinalIgnoreCase);
        private readonly HashSet<string> _files = new(StringComparer.OrdinalIgnoreCase);
        private readonly Dictionary<string, string> _links = new(StringComparer.OrdinalIgnoreCase);

        public void AddDirectory(string path)
        {
            for (string? p = Path.GetFullPath(path); p is not null; p = Path.GetDirectoryName(p))
                _directories.Add(p.TrimEnd('\\') is { Length: > 0 } t && t.Length > 2 ? t : p);
        }

        public void AddFile(string path)
        {
            _files.Add(Path.GetFullPath(path));
            if (Path.GetDirectoryName(Path.GetFullPath(path)) is { } parent) AddDirectory(parent);
        }

        /// <summary>Registers <paramref name="link"/> as a junction to <paramref name="target"/>.</summary>
        public void AddLink(string link, string target)
        {
            _links[Path.GetFullPath(link)] = Path.GetFullPath(target);
            _directories.Add(Path.GetFullPath(link));
        }

        public bool DirectoryExists(string path) => _directories.Contains(Normalize(path));

        public bool FileExists(string path) => _files.Contains(Normalize(path));

        public string ResolveFinalPath(string path)
        {
            var full = Normalize(path);
            for (var hop = 0; hop < 32; hop++)
            {
                if (!TryResolveOnce(full, out var next)) return full;
                full = next;
            }
            return full;
        }

        private bool TryResolveOnce(string full, out string resolved)
        {
            resolved = full;
            var suffix = "";
            for (string? current = full; current is not null; current = Path.GetDirectoryName(current))
            {
                if (_links.TryGetValue(current, out var target))
                {
                    resolved = suffix.Length == 0 ? target : Path.Combine(target, suffix);
                    return true;
                }
                var name = Path.GetFileName(current);
                if (name.Length == 0) return false;
                suffix = suffix.Length == 0 ? name : Path.Combine(name, suffix);
            }
            return false;
        }

        private static string Normalize(string path)
        {
            var full = Path.GetFullPath(path);
            // Keep drive roots as "C:\", trim the separator everywhere else.
            return full.Length > 3 ? full.TrimEnd('\\') : full;
        }
    }
}
