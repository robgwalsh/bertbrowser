using System.IO.Compression;
using System.Text;
using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ArchiveListingTests
{
    private static MemoryStream Zip(params (string Path, string Content)[] entries)
    {
        var stream = new MemoryStream();
        using (var archive = new ZipArchive(stream, ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var (path, content) in entries)
            {
                var entry = archive.CreateEntry(path);
                using var body = entry.Open();
                // Bytes, not a StreamWriter: the default UTF8 writer emits a byte-order mark, and
                // the sizes these tests assert on would silently be three bytes over.
                body.Write(Encoding.UTF8.GetBytes(content));
            }
        }
        stream.Position = 0;
        return stream;
    }

    [Fact]
    public void ListsEveryEntryWithItsSize()
    {
        using var zip = Zip(("readme.txt", "hello"), ("src/app.js", "console.log(1)"));
        var contents = ArchiveListing.Read(zip);

        Assert.Null(contents.Error);
        Assert.Equal(2, contents.TotalCount);
        Assert.Equal(2, contents.Entries.Count);
        Assert.Equal(5 + 14, contents.TotalBytes);
        Assert.Contains(contents.Entries, e => e.Path == "readme.txt" && e.SizeBytes == 5);
    }

    [Fact]
    public void SeparatorsAreWindowsSeparators() =>
        // The rest of the app writes paths one way; a preview should not be the exception.
        Assert.Contains(ArchiveListing.Read(Zip(("src/app.js", "x"))).Entries, e => e.Path == "src\\app.js");

    [Fact]
    public void EntriesComeBackInPathOrder()
    {
        using var zip = Zip(("zebra.txt", "z"), ("alpha.txt", "a"), ("Middle.txt", "m"));
        var paths = ArchiveListing.Read(zip).Entries.Select(e => e.Path).ToArray();
        string[] expected = ["alpha.txt", "Middle.txt", "zebra.txt"];
        Assert.Equal(expected, paths);
    }

    [Fact]
    public void AFolderEntryIsMarkedAsOneAndCarriesNoSize()
    {
        using var zip = Zip(("docs/", ""), ("docs/a.txt", "a"));
        var folder = Assert.Single(ArchiveListing.Read(zip).Entries, e => e.IsDirectory);
        Assert.Equal("docs\\", folder.Path);
        Assert.Equal(0, folder.SizeBytes);
    }

    [Fact]
    public void TheEntryCapTruncates_ButTheTotalStillCountsEverything()
    {
        var many = Enumerable.Range(0, 50).Select(i => ($"file{i:00}.txt", "x")).ToArray();
        using var zip = Zip(many);

        var contents = ArchiveListing.Read(zip, maxEntries: 10);
        Assert.True(contents.Truncated);
        Assert.Equal(10, contents.Entries.Count);
        Assert.Equal(50, contents.TotalCount);
        Assert.Equal(50, contents.TotalBytes);
    }

    [Fact]
    public void AnArchiveInsideTheCapIsNotReportedAsTruncated()
    {
        using var zip = Zip(("a.txt", "a"), ("b.txt", "b"));
        Assert.False(ArchiveListing.Read(zip, maxEntries: 2).Truncated);
    }

    [Fact]
    public void ADamagedArchiveIsAMessageRatherThanAThrow()
    {
        using var rubbish = new MemoryStream("this is not a zip file at all"u8.ToArray());
        var contents = ArchiveListing.Read(rubbish);

        Assert.NotNull(contents.Error);
        Assert.Empty(contents.Entries);
        Assert.Equal(0, contents.TotalCount);
    }

    [Fact]
    public void AnEmptyArchiveIsReadableAndEmpty()
    {
        using var zip = Zip();
        var contents = ArchiveListing.Read(zip);
        Assert.Null(contents.Error);
        Assert.Empty(contents.Entries);
        Assert.Equal(0, contents.CompressionRatio);
    }

    [Fact]
    public void TheStreamIsLeftOpenForTheCallerToClose()
    {
        using var zip = Zip(("a.txt", "a"));
        ArchiveListing.Read(zip);
        // Reading it twice must work — the listing owns nothing.
        Assert.Equal(1, ArchiveListing.Read(zip).TotalCount);
    }
}
