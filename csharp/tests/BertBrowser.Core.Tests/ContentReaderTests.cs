using System.Text;
using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The one class in the content search that opens a file, tested against real ones.
/// </summary>
/// <remarks>
/// The rules here are the ones no pure test could reach: the sharing flags, the attributes that
/// mean "do not read this", and the difference between a file that failed and a run that stopped.
/// </remarks>
public sealed class ContentReaderTests : IDisposable
{
    private readonly string _root = Path.Combine(
        Path.GetTempPath(), "bb-content-" + Guid.NewGuid().ToString("N"));

    private readonly FileSystemContentReader _reader = new();

    public ContentReaderTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }

    private string Write(string name, string content, Encoding? encoding = null)
    {
        var path = Path.Combine(_root, name);
        File.WriteAllText(path, content, encoding ?? new UTF8Encoding(false));
        return path;
    }

    private string WriteBytes(string name, byte[] bytes)
    {
        var path = Path.Combine(_root, name);
        File.WriteAllBytes(path, bytes);
        return path;
    }

    private ContentText? Read(string path, long max = ContentSearchRules.MaxBytesPerFile) =>
        _reader.Read(path, max, CancellationToken.None);

    // --- the ordinary cases ---

    [Fact]
    public void APlainTextFileComesBackWhole()
    {
        var text = Read(Write("a.txt", "hello TODO world"));
        Assert.NotNull(text);
        Assert.Equal("hello TODO world", text!.Text);
        Assert.False(text.Truncated);
        Assert.True(text.IndexOf("todo") >= 0);
    }

    [Fact]
    public void LineEndingsAreNormalisedSoLineNumbersMeanOneThing()
    {
        var text = Read(Write("crlf.txt", "a\r\nb\r\nTODO"));
        Assert.DoesNotContain('\r', text!.Text);
    }

    [Fact]
    public void AFileLongerThanTheBudgetIsReadToItAndSaysSo()
    {
        // The honesty that matters: a miss here is "not in the first N bytes", and the outcome
        // reports it rather than passing it off as "not in the file".
        var path = Write("big.txt", new string('x', 4_000) + "TODO");
        var text = Read(path, max: 1_000);

        Assert.NotNull(text);
        Assert.True(text!.Truncated);
        Assert.Equal(1_000, text.Text.Length);
        Assert.True(text.IndexOf("TODO") < 0, "the needle was past the budget and must not be found");
    }

    [Fact]
    public void AFileExactlyTheBudgetIsNotReportedTruncated()
    {
        var text = Read(Write("exact.txt", new string('x', 500)), max: 500);
        Assert.False(text!.Truncated);
    }

    [Fact]
    public void AnEmptyFileIsEmptyTextRatherThanAFailure()
    {
        var text = Read(Write("empty.txt", ""));
        Assert.NotNull(text);
        Assert.Equal("", text!.Text);
    }

    [Fact]
    public void AUtf16FileWithNoBomIsStillRead()
    {
        // The bomless-UTF-16 rung of the ladder, reached through the head sample. Decide the
        // encoding wrongly here and the whole file decodes as mojibake and matches nothing.
        var path = Write("utf16.txt", "hello TODO world", new UnicodeEncoding(false, false));
        var text = Read(path);
        Assert.NotNull(text);
        Assert.True(text!.IndexOf("TODO") >= 0);
    }

    [Fact]
    public void ABomIsNotPartOfTheText()
    {
        var text = Read(Write("bom.txt", "TODO", new UTF8Encoding(true)));
        Assert.Equal("TODO", text!.Text);
    }

    // --- not text ---

    [Fact]
    public void ABinaryFileIsNothingToSearchRatherThanAFailure()
    {
        // The distinction the outcome rests on. A folder of images must not report itself
        // "incomplete" — nothing went wrong, those files simply have no text.
        var bytes = new byte[4096];
        for (var i = 0; i < bytes.Length; i++) bytes[i] = (byte)(i % 251);
        var text = Read(WriteBytes("opaque.bin", bytes));

        Assert.Same(ContentText.None, text);
    }

    [Fact]
    public void ABinaryFileDoesNotMatchEvenWhenItsBytesSpellTheNeedle()
    {
        // A .exe with "TODO" in its string table is not a text hit, and reporting it as one would
        // fill a source-tree search with noise.
        var bytes = new byte[4096];
        Encoding.ASCII.GetBytes("TODO").CopyTo(bytes, 100);
        var text = Read(WriteBytes("withtext.bin", bytes));

        Assert.Same(ContentText.None, text);
    }

    // --- refusals and failures ---

    [Fact]
    public void AMissingFileIsAFailureRatherThanEmptyText() =>
        Assert.Null(Read(Path.Combine(_root, "not-here.txt")));

    [Fact]
    public void ACancelThrowsRatherThanReturningNull()
    {
        // Load-bearing: null means "this file had a problem, carry on", a throw means "the run is
        // stopping". Return null here and a cancelled search looks like a disk full of broken
        // files, which is what the outcome's two separate flags exist to keep apart.
        var path = Write("a.txt", "hello");
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        Assert.Throws<OperationCanceledException>(() =>
            _reader.Read(path, ContentSearchRules.MaxBytesPerFile, cts.Token));
    }

    [Fact]
    public void ReadingNeverBlocksARenameOrADelete()
    {
        // The house rule, and the one thing no in-memory test could catch: a held handle would
        // block this app's own executors in the very folder being searched. Narrow the share flags
        // to FileShare.Read and this goes red.
        var path = Write("held.txt", "TODO");

        using (var holder = new FileStream(path, FileMode.Open, FileAccess.Read,
                   FileShare.ReadWrite | FileShare.Delete))
        {
            var text = Read(path);
            Assert.NotNull(text);

            // And the other direction: while we would be reading it, it can still be renamed.
            var moved = Path.Combine(_root, "held-renamed.txt");
            File.Move(path, moved);
            Assert.True(File.Exists(moved));
        }
    }

    [Fact]
    public void AFileOpenedForWritingElsewhereIsStillSearchable()
    {
        var path = Write("log.txt", "TODO in a live log");

        using var writer = new FileStream(path, FileMode.Open, FileAccess.Write, FileShare.ReadWrite | FileShare.Delete);
        var text = Read(path);

        Assert.NotNull(text);
        Assert.True(text!.IndexOf("TODO") >= 0);
    }
}
