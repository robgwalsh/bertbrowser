using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// What a folder landing on a folder of the same name expands into. Everything here runs against a
/// fake listing rather than a real tree, so the rules can be stated at sizes and shapes — a
/// junction on the destination side, a two-thousand-folder overlap — that are impractical to build
/// on disk.
/// </summary>
public class TransferMergeExpanderTests
{
    private const string Src = @"C:\src";
    private const string Dst = @"C:\dst";

    // --- the shape of an expansion ---

    [Fact]
    public void AFolderClashingWithAFolder_GetsNoRowOfItsOwn()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg")
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg");

        var expanded = Expand(world, Folder("photos"));

        Assert.DoesNotContain(expanded.Transfers, t => t.SourcePath == @"C:\src\photos");
        Assert.Equal(["photos"], expanded.MergedFolders);
        Assert.True(expanded.IsMerged);
    }

    [Fact]
    public void OnlyTheDescendantsThatClash_AreAskedAbout()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos")
                .File(@"C:\src\photos\clashes.jpg")
                .File(@"C:\src\photos\brand-new.jpg")
            .Dir(@"C:\dst\photos")
                .File(@"C:\dst\photos\clashes.jpg");

        var expanded = Expand(world, Folder("photos"));

        Assert.Equal(2, expanded.Transfers.Count);
        var clash = Assert.Single(expanded.Conflicts);
        Assert.Equal(@"C:\src\photos\clashes.jpg", clash.SourcePath);
    }

    [Fact]
    public void AChildWithNoTwin_IsOneWholeItem_AndIsNotDescendedInto()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024")
                .File(@"C:\src\photos\2024\a.jpg").File(@"C:\src\photos\2024\b.jpg")
            .Dir(@"C:\dst\photos");

        var expanded = Expand(world, Folder("photos"));

        var only = Assert.Single(expanded.Transfers);
        Assert.Equal(@"C:\src\photos\2024", only.SourcePath);
        Assert.True(only.IsDirectory);
        Assert.False(only.Conflicts);
    }

    [Fact]
    public void TwoDirectoriesOnBothSides_AreOpenedRatherThanAskedAbout()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024").File(@"C:\src\photos\2024\a.jpg")
            .Dir(@"C:\dst\photos").Dir(@"C:\dst\photos\2024").File(@"C:\dst\photos\2024\a.jpg");

        var expanded = Expand(world, Folder("photos"));

        var only = Assert.Single(expanded.Transfers);
        Assert.Equal(@"C:\src\photos\2024\a.jpg", only.SourcePath);
        Assert.True(only.Conflicts);
    }

    [Fact]
    public void EveryEmittedDestination_HasAParentThatAlreadyExists()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024").Dir(@"C:\src\photos\2024\raw")
                .File(@"C:\src\photos\2024\raw\a.dng").File(@"C:\src\photos\2024\b.jpg")
            .Dir(@"C:\dst\photos").Dir(@"C:\dst\photos\2024").Dir(@"C:\dst\photos\2024\raw")
                .File(@"C:\dst\photos\2024\raw\a.dng");

        var expanded = Expand(world, Folder("photos"));

        Assert.NotEmpty(expanded.Transfers);
        foreach (var transfer in expanded.Transfers)
            Assert.True(
                world.DirectoryExists(Path.GetDirectoryName(transfer.DestinationPath)!),
                $"nothing may have to invent {Path.GetDirectoryName(transfer.DestinationPath)}");
    }

    [Fact]
    public void ANestedOverlap_ExpandsThroughEveryCommonLevel()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024").File(@"C:\src\photos\2024\deep.jpg")
            .Dir(@"C:\dst\photos").Dir(@"C:\dst\photos\2024").File(@"C:\dst\photos\2024\deep.jpg");

        var expanded = Expand(world, Folder("photos"));

        // Relative to where the drop lands, not to the merged folder — so a row names the folder it
        // came from as well as the file.
        Assert.Equal(@"photos\2024\deep.jpg", expanded.LabelFor(Assert.Single(expanded.Transfers)));
    }

    // --- what is not a merge ---

    [Fact]
    public void AFolderArrivingWhereAFileSits_IsOneClashingLeaf()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg")
            .File(@"C:\dst\photos");

        var expanded = Expand(world, Folder("photos"));

        var only = Assert.Single(expanded.Transfers);
        Assert.Equal(@"C:\src\photos", only.SourcePath);
        Assert.True(only.Conflicts);
        Assert.False(expanded.IsMerged);
    }

    [Fact]
    public void AFileArrivingWhereAFolderSits_IsOneClashingLeaf()
    {
        var world = new FakeEntrySource()
            .File(@"C:\src\notes").Dir(@"C:\dst\notes").File(@"C:\dst\notes\inner.txt");

        var expanded = Expand(world, new PlannedTransfer(@"C:\src\notes", false, @"C:\dst\notes", true));

        Assert.Single(expanded.Transfers);
        Assert.False(expanded.IsMerged);
    }

    [Fact]
    public void AFileClashWithNoFolderInvolved_IsLeftAlone_ButDescribed()
    {
        var world = new FakeEntrySource()
            .File(@"C:\src\a.txt", size: 10).File(@"C:\dst\a.txt", size: 20);

        var expanded = Expand(world, new PlannedTransfer(@"C:\src\a.txt", false, @"C:\dst\a.txt", true));

        var only = Assert.Single(expanded.Transfers);
        Assert.NotNull(only.Clash);
        Assert.Equal(20, only.Clash!.Existing!.Value.Bytes);
    }

    [Fact]
    public void APlanWithNothingClashing_IsReturnedUntouched()
    {
        var world = new FakeEntrySource().Dir(@"C:\src\photos").Dir(@"C:\dst");
        var plan = Plan(new PlannedTransfer(@"C:\src\photos", true, @"C:\dst\photos", false));

        Assert.Same(plan, new TransferMergeExpander(world).Expand(plan));
    }

    // --- links are always leaves ---

    [Fact]
    public void ASourceFolderThatIsAJunction_IsNeverDescendedInto()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos", link: true).File(@"C:\src\photos\a.jpg")
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg");

        var expanded = Expand(world, Folder("photos"));

        Assert.Equal(@"C:\src\photos", Assert.Single(expanded.Transfers).SourcePath);
        Assert.False(expanded.IsMerged);
    }

    /// <summary>
    /// The dangerous one. A junction at the destination pointing back into the source would be
    /// merged into one file at a time, by a path <c>Revalidate</c> never inspects — it compares a
    /// source against the plan's destination directory, and an expanded source is always deeper.
    /// </summary>
    [Fact]
    public void ADestinationFolderThatIsAJunction_IsNeverMergedInto()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg")
            .Dir(@"C:\dst\photos", link: true).File(@"C:\dst\photos\a.jpg");

        var expanded = Expand(world, Folder("photos"));

        Assert.Equal(@"C:\src\photos", Assert.Single(expanded.Transfers).SourcePath);
        Assert.False(expanded.IsMerged);
    }

    [Fact]
    public void AJunctionInsideAMergedFolder_IsAnItemRatherThanASubtree()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\link", link: true)
                .File(@"C:\src\photos\link\a.jpg")
            .Dir(@"C:\dst\photos").Dir(@"C:\dst\photos\link");

        var expanded = Expand(world, Folder("photos"));

        var only = Assert.Single(expanded.Transfers);
        Assert.Equal(@"C:\src\photos\link", only.SourcePath);
        Assert.True(only.Conflicts);
    }

    // --- two folders merging into one ---

    [Fact]
    public void TwoFoldersOfTheSameNameMergingIntoOne_FlagTheSecondClaimant()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\a\photos").File(@"C:\a\photos\x.jpg")
            .Dir(@"C:\b\photos").File(@"C:\b\photos\x.jpg")
            .Dir(@"C:\dst\photos");

        var plan = Plan(
            new PlannedTransfer(@"C:\a\photos", true, @"C:\dst\photos", true),
            new PlannedTransfer(@"C:\b\photos", true, @"C:\dst\photos", true));

        var expanded = new TransferMergeExpander(world).Expand(plan);

        Assert.Equal(2, expanded.Transfers.Count);
        Assert.False(expanded.Transfers[0].Conflicts);
        Assert.True(expanded.Transfers[1].Conflicts);
        Assert.NotNull(expanded.EarlierClaimantOf(expanded.Transfers[1]));
    }

    // --- pruning ---

    [Fact]
    public void EveryOpenedSourceFolder_IsListedForPruning_DeepestFirst()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024").File(@"C:\src\photos\2024\a.jpg")
            .Dir(@"C:\dst\photos").Dir(@"C:\dst\photos\2024").File(@"C:\dst\photos\2024\a.jpg");

        var expanded = Expand(world, Folder("photos"));

        Assert.Equal([@"C:\src\photos\2024", @"C:\src\photos"], expanded.PruneDirectories);
    }

    [Fact]
    public void AFolderThatWasNotMerged_ContributesNothingToPruning()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg")
            .File(@"C:\dst\photos");

        Assert.Empty(Expand(world, Folder("photos")).PruneDirectories);
    }

    // --- identical ---

    [Fact]
    public void AByteIdenticalClash_IsMarkedIdentical_AndDefaultsToSkip()
    {
        var when = new DateTime(2026, 3, 1, 12, 0, 0, DateTimeKind.Utc);
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 900, modified: when)
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg", size: 900, modified: when);

        var only = Assert.Single(Expand(world, Folder("photos")).Transfers);

        Assert.True(only.Clash!.Identical);
        Assert.Equal(ConflictResolution.Skip, ConflictDefaults.For(only));
    }

    [Fact]
    public void ADifferentSizeAtTheSameInstant_IsNotIdentical()
    {
        var when = new DateTime(2026, 3, 1, 12, 0, 0, DateTimeKind.Utc);
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 900, modified: when)
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg", size: 901, modified: when);

        var only = Assert.Single(Expand(world, Folder("photos")).Transfers);

        Assert.False(only.Clash!.Identical);
        Assert.Equal(ConflictResolution.KeepBoth, ConflictDefaults.For(only));
    }

    /// <summary>FAT and exFAT round a write time to two seconds, so a file copied to a USB stick
    /// comes back up to that much off and must not read as "newer" for ever.</summary>
    [Fact]
    public void TwoSecondsApartAtTheSameSize_IsStillIdentical()
    {
        var when = new DateTime(2026, 3, 1, 12, 0, 0, DateTimeKind.Utc);
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 900, modified: when.AddSeconds(2))
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg", size: 900, modified: when);

        Assert.True(Assert.Single(Expand(world, Folder("photos")).Transfers).Clash!.Identical);
    }

    /// <summary>
    /// The load-bearing one. A folder comparison forgives a whole-hour shift, because FAT stores
    /// local time with no zone. Forgiving it here would pre-select Skip on a file that really is an
    /// hour newer — the user asks for a merge and silently does not get one.
    /// </summary>
    [Fact]
    public void AWholeHourNewer_IsNotIdentical_AndIsReportedAsNewer()
    {
        var when = new DateTime(2026, 3, 1, 12, 0, 0, DateTimeKind.Utc);
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 900, modified: when.AddHours(1))
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg", size: 900, modified: when);

        var clash = Assert.Single(Expand(world, Folder("photos")).Transfers).Clash!;

        Assert.False(clash.Identical);
        Assert.True(clash.IncomingIsNewer);
    }

    [Fact]
    public void AMissingTimestamp_IsNeverIdentical()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 900, modified: DateTime.MinValue)
            .Dir(@"C:\dst\photos").File(@"C:\dst\photos\a.jpg", size: 900, modified: DateTime.MinValue);

        Assert.False(Assert.Single(Expand(world, Folder("photos")).Transfers).Clash!.Identical);
    }

    // --- sizes come free ---

    [Fact]
    public void AnExpandedFile_CarriesTheSizeTheWalkAlreadyRead()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").File(@"C:\src\photos\a.jpg", size: 4096)
            .Dir(@"C:\dst\photos");

        Assert.Equal(4096, Assert.Single(Expand(world, Folder("photos")).Transfers).KnownBytes);
    }

    [Fact]
    public void AnExpandedFolder_CarriesNoSize_BecauseThatComesFromTheIndex()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\src\photos\2024").File(@"C:\src\photos\2024\a.jpg")
            .Dir(@"C:\dst\photos");

        Assert.Null(Assert.Single(Expand(world, Folder("photos")).Transfers).KnownBytes);
    }

    // --- the ceilings ---

    [Fact]
    public void OverTheItemCeiling_TheFolderKeepsItsWholesaleQuestion()
    {
        var world = new FakeEntrySource().Dir(@"C:\src\photos").Dir(@"C:\dst\photos");
        for (var i = 0; i <= TransferMergeLimits.MaxExpandedTransfers; i++)
            world.File($@"C:\src\photos\f{i}.txt");

        var expanded = Expand(world, Folder("photos"));

        var only = Assert.Single(expanded.Transfers);
        Assert.Equal(@"C:\src\photos", only.SourcePath);
        Assert.True(only.MergeDeclined);
        Assert.False(expanded.IsMerged);
        Assert.Empty(expanded.PruneDirectories);
    }

    [Fact]
    public void OverTheDirectoryCeiling_TheFolderKeepsItsWholesaleQuestion()
    {
        var world = new FakeEntrySource().Dir(@"C:\src\photos").Dir(@"C:\dst\photos");
        for (var i = 0; i <= TransferMergeLimits.MaxDirectoriesVisited; i++)
        {
            world.Dir($@"C:\src\photos\d{i}").Dir($@"C:\dst\photos\d{i}");
            world.File($@"C:\src\photos\d{i}\f.txt");
        }

        Assert.True(Assert.Single(Expand(world, Folder("photos")).Transfers).MergeDeclined);
    }

    [Fact]
    public void OneFolderMayDecline_WhileAnotherMerges()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\big").Dir(@"C:\dst\big")
            .Dir(@"C:\src\small").File(@"C:\src\small\a.txt")
            .Dir(@"C:\dst\small").File(@"C:\dst\small\a.txt");
        for (var i = 0; i <= TransferMergeLimits.MaxExpandedTransfers; i++)
            world.File($@"C:\src\big\f{i}.txt");

        var expanded = new TransferMergeExpander(world).Expand(Plan(
            new PlannedTransfer(@"C:\src\big", true, @"C:\dst\big", true),
            new PlannedTransfer(@"C:\src\small", true, @"C:\dst\small", true)));

        Assert.Equal(["small"], expanded.MergedFolders);
        Assert.Contains(expanded.Transfers, t => t is { SourcePath: @"C:\src\big", MergeDeclined: true });
        Assert.Contains(expanded.Transfers, t => t.SourcePath == @"C:\src\small\a.txt");
    }

    // --- degenerate input ---

    [Fact]
    public void AnUnreadableFolder_BecomesALeafRatherThanThrowing()
    {
        var world = new FakeEntrySource()
            .Dir(@"C:\src\photos").Dir(@"C:\dst\photos");

        // Nothing inside either side: the walk opens both, finds nothing, and emits nothing.
        var expanded = Expand(world, Folder("photos"));

        Assert.Empty(expanded.Transfers);
        Assert.Equal([@"C:\src\photos"], expanded.PruneDirectories);
    }

    // --- helpers ---

    private static PlannedTransfer Folder(string name) =>
        new($@"{Src}\{name}", true, $@"{Dst}\{name}", true);

    private static TransferPlan Plan(params PlannedTransfer[] transfers) =>
        new(TransferVerb.Copy, Dst, transfers, []);

    private static TransferPlan Expand(FakeEntrySource world, PlannedTransfer transfer) =>
        new TransferMergeExpander(world).Expand(Plan(transfer));

    /// <summary>
    /// A filesystem described rather than created. Registering a path registers its ancestors as
    /// directories, so a test reads as the tree it means.
    /// </summary>
    private sealed class FakeEntrySource : ITransferEntrySource
    {
        private readonly Dictionary<string, TransferEntry> _byPath = new(StringComparer.OrdinalIgnoreCase);

        public FakeEntrySource Dir(string path, bool link = false)
        {
            Add(path, isDirectory: true, size: 0, modified: default, link);
            return this;
        }

        public FakeEntrySource File(string path, long size = 1, DateTime? modified = null)
        {
            Add(path, isDirectory: false, size, modified ?? new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc),
                link: false);
            return this;
        }

        private void Add(string path, bool isDirectory, long size, DateTime modified, bool link)
        {
            if (Path.GetDirectoryName(path) is { Length: > 0 } parent && !_byPath.ContainsKey(parent))
                Add(parent, isDirectory: true, 0, default, link: false);

            _byPath[path] = new TransferEntry(
                Path.GetFileName(path), path, isDirectory, link, size, modified);
        }

        public bool DirectoryExists(string path) =>
            _byPath.TryGetValue(path, out var e) ? e.IsDirectory : IsRoot(path);

        public bool FileExists(string path) => _byPath.TryGetValue(path, out var e) && !e.IsDirectory;

        public string ResolveFinalPath(string path) => path;

        public IReadOnlyList<TransferEntry> Entries(string directory) =>
        [
            .. _byPath.Values.Where(e =>
                string.Equals(Path.GetDirectoryName(e.FullPath), directory, StringComparison.OrdinalIgnoreCase)),
        ];

        private static bool IsRoot(string path) => Path.GetDirectoryName(path) is null;
    }
}
