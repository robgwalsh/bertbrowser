using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class MftDirectorySizeBuilderTests
{
    private const ulong Root = 5;

    private static MftFileRecord Dir(ulong rec, ulong parent, string name, bool hidden = false) =>
        new(rec, parent, name, IsDirectory: true, Hidden: hidden, Size: 0, ModifiedUtc: DateTime.UnixEpoch);

    private static MftFileRecord File(ulong rec, ulong parent, string name, long size) =>
        new(rec, parent, name, IsDirectory: false, Hidden: false, Size: size, ModifiedUtc: DateTime.UnixEpoch);

    [Fact]
    public void Build_RollsSizesAndCountsUpTheTree()
    {
        var b = new MftDirectorySizeBuilder();
        // C:\Users\Rob\{a.txt=100, b.txt=200}; C:\Users\c.txt=50; C:\Users\Junction (empty, e.g. reparse)
        b.Add(Dir(20, Root, "Users"));
        b.Add(Dir(21, 20, "Rob"));
        b.Add(Dir(40, 20, "Junction"));
        b.Add(File(30, 21, "a.txt", 100));
        b.Add(File(31, 21, "b.txt", 200));
        b.Add(File(32, 20, "c.txt", 50));

        var results = b.Build(@"C:\", DateTime.UnixEpoch, new());
        var byKey = results.ToDictionary(r => r.PathKey);

        Assert.Equal(300, byKey[@"C:\USERS\ROB"].SizeBytes);
        Assert.Equal(2, byKey[@"C:\USERS\ROB"].FileCount);
        Assert.Equal(0, byKey[@"C:\USERS\ROB"].DirCount);

        Assert.Equal(350, byKey[@"C:\USERS"].SizeBytes);   // 300 under Rob + 50 direct
        Assert.Equal(3, byKey[@"C:\USERS"].FileCount);
        Assert.Equal(2, byKey[@"C:\USERS"].DirCount);      // Rob + Junction

        // A childless directory (junction target lives under its own record) contributes nothing.
        Assert.Equal(0, byKey[@"C:\USERS\JUNCTION"].SizeBytes);
        Assert.Equal(0, byKey[@"C:\USERS\JUNCTION"].FileCount);
    }

    [Fact]
    public void Build_PopulatesDirCacheAndMarksComplete()
    {
        var b = new MftDirectorySizeBuilder();
        b.Add(Dir(20, Root, "Data"));
        b.Add(File(30, 20, "x.bin", 10));

        var cache = new Dictionary<ulong, (string Path, bool Hidden)>();
        var results = b.Build(@"D:\", DateTime.UnixEpoch, cache);

        Assert.Equal(@"D:\Data", cache[20].Path); // reused to build fs_entry file rows
        Assert.All(results, r => Assert.False(r.Incomplete)); // MFT sees everything, never partial
    }

    [Fact]
    public void Build_SkipsSubtreesWhoseParentChainIsBroken()
    {
        // A directory whose parent was never added (e.g. under a skipped $Extend) is unreachable
        // from the root and must not appear.
        var b = new MftDirectorySizeBuilder();
        b.Add(Dir(50, 11 /* not added */, "Orphan"));
        b.Add(File(60, 50, "y.bin", 999));

        var results = b.Build(@"C:\", DateTime.UnixEpoch, new());

        Assert.DoesNotContain(results, r => r.PathKey.Contains("ORPHAN"));
    }
}
