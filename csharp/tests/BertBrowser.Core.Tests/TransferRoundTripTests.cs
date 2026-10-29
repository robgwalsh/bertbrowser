using System.Security.Cryptography;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// Property tests over randomly generated trees and randomly chosen drops. Rather than checking
/// specific behaviours, these assert the two invariants the feature exists to hold:
/// <list type="number">
/// <item>A move never changes which file contents exist anywhere under the working root. Files
/// change location; nothing is created, truncated, or destroyed.</item>
/// <item>Undoing a move restores the tree byte-for-byte, including anything a Replace displaced.</item>
/// </list>
/// Several seeds are run so each shape — nested selections, name collisions, folders dropped near
/// their own subtree — comes up across the matrix.
/// </summary>
public sealed class TransferRoundTripTests : IDisposable
{
    private readonly string _root;
    private readonly TransferPlanner _planner = new();
    private readonly TransferExecutor _executor = new();

    public TransferRoundTripTests()
    {
        _root = Path.Combine(Path.GetTempPath(), $"bertbrowser-roundtrip-{Guid.NewGuid():N}");
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

    [Theory]
    [InlineData(1)]
    [InlineData(2)]
    [InlineData(3)]
    [InlineData(5)]
    [InlineData(8)]
    [InlineData(13)]
    [InlineData(21)]
    [InlineData(34)]
    public void ASequenceOfMoves_NeverLosesAnyFileContent(int seed)
    {
        var rng = new Random(seed);
        BuildTree(rng);

        var expected = ContentCounts();
        Assert.NotEmpty(expected);

        for (var round = 0; round < 8; round++)
        {
            var resolution = (ConflictResolution)rng.Next(3);
            PerformRandomDrop(rng, TransferVerb.Move, resolution);

            // The multiset of file contents under the root is invariant under a move: every byte
            // that was there is still there, wherever it now lives (staging included).
            Assert.Equal(expected, ContentCounts());
        }
    }

    [Theory]
    [InlineData(1)]
    [InlineData(2)]
    [InlineData(3)]
    [InlineData(5)]
    [InlineData(8)]
    [InlineData(13)]
    [InlineData(21)]
    [InlineData(34)]
    public void AMove_ThenUndo_RestoresTheTreeExactly(int seed)
    {
        var rng = new Random(seed);
        BuildTree(rng);

        var before = Snapshot();

        var outcome = PerformRandomDrop(rng, TransferVerb.Move, (ConflictResolution)rng.Next(3));
        if (outcome is null || outcome.Completed.Count == 0) return; // this seed produced no valid drop

        Assert.NotEqual(before, Snapshot()); // something really did move

        var undo = _executor.Undo(outcome);

        Assert.Empty(undo.Failed);
        Assert.Equal(before, Snapshot());
    }

    [Theory]
    [InlineData(4)]
    [InlineData(7)]
    [InlineData(11)]
    public void ACopy_AddsWithoutDisturbingWhatWasAlreadyThere(int seed)
    {
        var rng = new Random(seed);
        BuildTree(rng);

        var before = ContentCounts();
        PerformRandomDrop(rng, TransferVerb.Copy, (ConflictResolution)rng.Next(3));
        var after = ContentCounts();

        // A copy is purely additive: every original content is still present, at least as often.
        foreach (var (hash, count) in before)
            Assert.True(after.TryGetValue(hash, out var now) && now >= count,
                $"content {hash[..8]} went from {count} occurrences to {(after.TryGetValue(hash, out var n) ? n : 0)}");
    }

    // --- random world building ---

    private void BuildTree(Random rng)
    {
        // A handful of folders, deliberately including repeated names so drops collide, and nested
        // ones so a selection can contain both a folder and something inside it.
        string[] folderNames = ["alpha", "beta", "gamma", "shared", "alpha"];
        var directories = new List<string> { _root };

        for (var i = 0; i < 10; i++)
        {
            var parent = directories[rng.Next(directories.Count)];
            var path = Path.Combine(parent, folderNames[rng.Next(folderNames.Length)]);
            if (Directory.Exists(path)) path += $"-{i}";
            Directory.CreateDirectory(path);
            directories.Add(path);
        }

        string[] fileNames = ["notes.txt", "data.bin", "readme.md", "notes.txt", "image.png"];
        for (var i = 0; i < 30; i++)
        {
            var parent = directories[rng.Next(directories.Count)];
            var name = fileNames[rng.Next(fileNames.Length)];
            var path = Path.Combine(parent, name);
            if (File.Exists(path)) path = Path.Combine(parent, $"{i}-{name}");
            File.WriteAllText(path, $"content-{i}-{rng.Next()}");
        }
    }

    /// <summary>Picks a random handful of entries and a random destination folder, plans it, and
    /// executes whatever the planner allowed. Returns null when the planner refused everything.</summary>
    private TransferOutcome? PerformRandomDrop(Random rng, TransferVerb verb, ConflictResolution resolution)
    {
        var entries = Directory.GetFileSystemEntries(_root, "*", SearchOption.AllDirectories)
            .Where(p => !IsInStaging(p))
            .ToList();
        var folders = entries.Where(Directory.Exists).Append(_root).ToList();
        if (entries.Count == 0 || folders.Count == 0) return null;

        var sources = Enumerable.Range(0, rng.Next(1, 5))
            .Select(_ => entries[rng.Next(entries.Count)])
            .Distinct()
            .ToList();
        var destination = folders[rng.Next(folders.Count)];

        var plan = _planner.Plan(sources, destination, verb);
        if (!plan.HasWork) return null;

        var resolutions = plan.Transfers.ToDictionary(
            t => PathKey.Canonicalize(t.SourcePath), _ => resolution);
        var outcome = _executor.Execute(plan, resolutions);

        // A refusal is fine; a crash-shaped failure is not.
        Assert.All(outcome.Failed, f => Assert.False(string.IsNullOrWhiteSpace(f.Message)));
        return outcome;
    }

    // --- observation ---

    /// <summary>How many files hold each distinct content, anywhere under the root. Location-free,
    /// so it is unchanged by a move and only grows on a copy.</summary>
    private Dictionary<string, int> ContentCounts()
    {
        var counts = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var file in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
        {
            var hash = HashOf(file);
            counts[hash] = counts.TryGetValue(hash, out var n) ? n + 1 : 1;
        }
        return counts;
    }

    /// <summary>Every file's path and content, plus every folder, as one comparable string. Staging
    /// folders are included: after a clean undo there should be none left.</summary>
    private string Snapshot()
    {
        var lines = new List<string>();
        foreach (var file in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
            lines.Add($"F {Path.GetRelativePath(_root, file)} {HashOf(file)}");
        foreach (var dir in Directory.EnumerateDirectories(_root, "*", SearchOption.AllDirectories))
            lines.Add($"D {Path.GetRelativePath(_root, dir)}");

        lines.Sort(StringComparer.Ordinal);
        return string.Join('\n', lines);
    }

    private static string HashOf(string path)
    {
        using var stream = File.OpenRead(path);
        return Convert.ToHexString(SHA256.HashData(stream));
    }

    private static bool IsInStaging(string path) =>
        path.Contains(".bertbrowser-replaced-", StringComparison.Ordinal);

    /// <summary>Guards the guard: if the hash-multiset check could not tell a lost file from an
    /// intact tree, every other assertion in this class would be worthless.</summary>
    [Fact]
    public void TheContentInvariant_ActuallyDetectsALostFile()
    {
        Directory.CreateDirectory(Path.Combine(_root, "folder"));
        File.WriteAllText(Path.Combine(_root, "folder", "a.txt"), "payload");
        File.WriteAllText(Path.Combine(_root, "b.txt"), "other");

        var before = ContentCounts();
        File.Delete(Path.Combine(_root, "folder", "a.txt"));

        Assert.NotEqual(before, ContentCounts());
    }

    /// <summary>Same for the snapshot: a file moved to a different folder must show up as a change.</summary>
    [Fact]
    public void TheSnapshot_ActuallyDetectsAMovedFile()
    {
        Directory.CreateDirectory(Path.Combine(_root, "folder"));
        File.WriteAllText(Path.Combine(_root, "a.txt"), "payload");

        var before = Snapshot();
        File.Move(Path.Combine(_root, "a.txt"), Path.Combine(_root, "folder", "a.txt"));

        Assert.NotEqual(before, Snapshot());
    }
}
