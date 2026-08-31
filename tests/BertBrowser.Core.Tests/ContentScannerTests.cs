using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Search;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The reading pass: which candidates it opens, which it does not, and what it reports when it is
/// stopped or runs out of budget.
/// </summary>
/// <remarks>
/// The reader is faked throughout — not for speed but for determinism. A cancel has to land in the
/// middle of the run every time, which against a real disk would mean writing a very large fixture
/// and racing it; this is the <c>SteppedCopier</c>/<c>FakeHasher</c> pattern the transfer and
/// duplicate tests already use.
/// </remarks>
public sealed class ContentScannerTests
{
    private static SearchQuery Q(string text)
    {
        var parse = SearchQuery.Parse(text);
        Assert.Null(parse.Problem);
        Assert.NotNull(parse.Query);
        return parse.Query!;
    }

    private static SearchHit Hit(string name, bool isDir = false) =>
        new(@"C:\Data\" + name, "", name, isDir, 100, new DateTime(2026, 6, 1));

    /// <summary>A reader over a dictionary, with a hook that runs before each file is answered.</summary>
    private sealed class FakeReader(Dictionary<string, string?> files, Action<string>? before = null)
        : IContentReader
    {
        public List<string> Opened { get; } = [];

        public ContentText? Read(string path, long maxBytes, CancellationToken ct)
        {
            ct.ThrowIfCancellationRequested();
            before?.Invoke(path);
            lock (Opened) Opened.Add(path);

            if (!files.TryGetValue(Path.GetFileName(path), out var text)) return null;
            if (text is null) return ContentText.None;

            var truncated = text.Length > maxBytes;
            return new ContentText(truncated ? text[..(int)maxBytes] : text, truncated);
        }
    }

    private static ContentScanOutcome Run(
        IContentReader reader, SearchQuery query, IReadOnlyList<SearchHit> candidates,
        int maxResults = 1000, bool candidatesTruncated = false, CancellationToken ct = default) =>
        new ContentScanner(reader).Scan(
            query, candidates, maxResults, candidatesTruncated,
            ContentSearchRules.MaxBytesPerFile, null, null, ct);

    // --- what gets opened ---

    [Fact]
    public void OnlyMatchingFilesSurvive()
    {
        var reader = new FakeReader(new() { ["a.cs"] = "has TODO here", ["b.cs"] = "clean" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.cs"), Hit("b.cs")]);

        Assert.Equal(["a.cs"], outcome.Hits.Select(h => h.Name));
        Assert.Equal(2, outcome.Report.FilesRead);
    }

    [Fact]
    public void ACandidateTheNameAlreadySettledIsNeverOpened()
    {
        // The budget argument, asserted. In "content:zzz OR ext:md" the .md is a hit on its name,
        // and opening it would spend the file ceiling re-establishing what was already known.
        // Make OrNode return NeedsContent past a settled Yes and this goes red.
        var reader = new FakeReader(new() { ["a.cs"] = "nothing" });
        var outcome = Run(reader, Q("content:zzz OR ext:md"), [Hit("notes.md"), Hit("a.cs")]);

        Assert.Equal(["notes.md"], outcome.Hits.Select(h => h.Name));
        Assert.DoesNotContain(reader.Opened, p => p.EndsWith("notes.md"));
        Assert.Equal(1, outcome.Report.FilesRead);
    }

    [Fact]
    public void ADirectoryIsNeverHandedToTheReader()
    {
        // Directories are settled No by the term itself, so the reader must not see one at all.
        var reader = new FakeReader(new() { ["a.cs"] = "TODO" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("sub", isDir: true), Hit("a.cs")]);

        Assert.Equal(["a.cs"], outcome.Hits.Select(h => h.Name));
        Assert.DoesNotContain(reader.Opened, p => p.EndsWith("sub"));
    }

    [Fact]
    public void AConjunctionThatIsAlreadyRefusedOpensNothingAtAll()
    {
        // "is:dir content:x" — every candidate is settled No before any I/O.
        var reader = new FakeReader(new() { ["a.cs"] = "TODO" });
        var outcome = Run(reader, Q("is:dir content:TODO"), [Hit("sub", isDir: true), Hit("a.cs")]);

        Assert.Empty(outcome.Hits);
        Assert.Empty(reader.Opened);
    }

    [Fact]
    public void ANegatedContentTermIsAnsweredByReadingToo()
    {
        var reader = new FakeReader(new() { ["a.cs"] = "has TODO", ["b.cs"] = "clean" });
        var outcome = Run(reader, Q("ext:cs !content:TODO"), [Hit("a.cs"), Hit("b.cs")]);

        Assert.Equal(["b.cs"], outcome.Hits.Select(h => h.Name));
    }

    // --- the snippet ---

    [Fact]
    public void AHitCarriesTheLineItMatchedOn()
    {
        var reader = new FakeReader(new() { ["a.cs"] = "one\ntwo\nthree TODO four" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.cs")]);

        var match = Assert.IsType<ContentMatch>(outcome.Hits[0].Match);
        Assert.Equal(3, match.LineNumber);
        Assert.Equal("three TODO four", match.Line);
    }

    [Fact]
    public void AHitSettledByItsNameCarriesNoLine()
    {
        // There is no line to point at — the file was never opened. Inventing one would be a lie
        // about where the match was.
        var reader = new FakeReader(new() { ["a.cs"] = "x" });
        var outcome = Run(reader, Q("content:zzz OR ext:md"), [Hit("notes.md")]);

        Assert.Null(outcome.Hits[0].Match);
    }

    // --- failures vs cancels ---

    [Fact]
    public void OneUnreadableFileCostsTheOthersNothing()
    {
        var reader = new FakeReader(new() { ["b.cs"] = "TODO here" }); // a.cs missing => null
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.cs"), Hit("b.cs")]);

        Assert.Equal(["b.cs"], outcome.Hits.Select(h => h.Name));
        Assert.Equal(1, outcome.Report.Unreadable);
        Assert.True(outcome.Report.Incomplete);
        Assert.False(outcome.Cancelled);
    }

    [Fact]
    public void ABinaryCandidateIsNotCountedAsUnreadable()
    {
        // Nothing went wrong: it simply has no text. Counting it would make every search of a
        // folder with images report itself incomplete, and the word would stop meaning anything.
        var reader = new FakeReader(new() { ["a.png"] = null, ["b.cs"] = "TODO" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.png"), Hit("b.cs")]);

        Assert.Equal(["b.cs"], outcome.Hits.Select(h => h.Name));
        Assert.Equal(0, outcome.Report.Unreadable);
    }

    [Fact]
    public void ACancelMidRunGivesBackWhatWasAlreadyFound()
    {
        // A floor, not nothing. Return an empty list on a cancel and the results the user was
        // watching fill up are blanked at the moment they press Escape.
        using var cts = new CancellationTokenSource();
        var files = Enumerable.Range(0, 200).ToDictionary(i => $"f{i}.cs", _ => (string?)"TODO here");
        var seen = 0;
        var reader = new FakeReader(files, _ =>
        {
            if (Interlocked.Increment(ref seen) == 20) cts.Cancel();
        });

        var candidates = Enumerable.Range(0, 200).Select(i => Hit($"f{i}.cs")).ToList();
        var outcome = Run(reader, Q("content:TODO"), candidates, ct: cts.Token);

        Assert.True(outcome.Cancelled);
        Assert.NotEmpty(outcome.Hits);
        Assert.True(outcome.Hits.Count < 200, "a cancelled run must be short of the whole set");
    }

    [Fact]
    public void ACancelIsNotReportedAsAPileOfUnreadableFiles()
    {
        using var cts = new CancellationTokenSource();
        var files = Enumerable.Range(0, 100).ToDictionary(i => $"f{i}.cs", _ => (string?)"TODO");
        var reader = new FakeReader(files, _ => cts.Cancel());
        var candidates = Enumerable.Range(0, 100).Select(i => Hit($"f{i}.cs")).ToList();

        var outcome = Run(reader, Q("content:TODO"), candidates, ct: cts.Token);

        Assert.True(outcome.Cancelled);
        Assert.Equal(0, outcome.Report.Unreadable);
    }

    // --- ceilings ---

    [Fact]
    public void ReachingTheCandidateCeilingIsReportedRatherThanLookingLikeNoMoreResults()
    {
        var reader = new FakeReader(new() { ["a.cs"] = "TODO" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.cs")], candidatesTruncated: true);

        Assert.Equal(ContentScanLimit.Candidates, outcome.Report.Limit);
        Assert.True(outcome.Report.Incomplete);
    }

    [Fact]
    public void TheReportNamesWhereItGotTo()
    {
        // "Searched the first 50,000 files" is true and still reads as "your PC has no such file".
        // Naming the last path examined is what makes it a bound rather than a lie.
        var reader = new FakeReader(new() { ["a.cs"] = "TODO", ["b.cs"] = "TODO" });
        var outcome = Run(reader, Q("content:TODO"), [Hit("a.cs"), Hit("b.cs")]);

        Assert.NotNull(outcome.Report.LastPathExamined);
    }

    [Fact]
    public void AFileLongerThanItsBudgetIsCountedSoTheAnswerIsKnownToBeAFloor()
    {
        var reader = new FakeReader(new() { ["big.cs"] = new string('x', (int)ContentSearchRules.MaxBytesPerFile + 10) });
        var outcome = Run(reader, Q("content:TODO"), [Hit("big.cs")]);

        Assert.Equal(1, outcome.Report.Truncated);
        Assert.True(outcome.Report.Incomplete);
    }

    [Fact]
    public void TheResultCapIsSeparateFromTheFileCeiling()
    {
        var files = Enumerable.Range(0, 50).ToDictionary(i => $"f{i}.cs", _ => (string?)"TODO");
        var reader = new FakeReader(files);
        var candidates = Enumerable.Range(0, 50).Select(i => Hit($"f{i}.cs")).ToList();

        var outcome = Run(reader, Q("content:TODO"), candidates, maxResults: 10);

        Assert.Equal(10, outcome.Hits.Count);
        Assert.True(outcome.Truncated);
        Assert.Equal(50, outcome.Report.FilesRead); // every candidate was still examined
    }

    // --- determinism ---

    [Fact]
    public void TwoIdenticalRunsReturnTheSameOrder()
    {
        // Parallel.ForEach completes in whatever order the disk answers. A result list that
        // reshuffled between two identical searches would be impossible to act on.
        var files = Enumerable.Range(0, 60).ToDictionary(i => $"f{i:D2}.cs", _ => (string?)"TODO");
        var candidates = Enumerable.Range(0, 60).Select(i => Hit($"f{i:D2}.cs")).ToList();

        var first = Run(new FakeReader(files), Q("content:TODO"), candidates);
        var second = Run(new FakeReader(files), Q("content:TODO"), candidates.AsEnumerable().Reverse().ToList());

        Assert.Equal(first.Hits.Select(h => h.Name), second.Hits.Select(h => h.Name));
    }

    // --- progress ---

    [Fact]
    public void ProgressAlwaysReportsAtTheFinish()
    {
        var reports = new List<ContentScanProgress>();
        var reader = new FakeReader(new() { ["a.cs"] = "TODO" });

        new ContentScanner(reader).Scan(
            Q("content:TODO"), [Hit("a.cs")], 1000, false, ContentSearchRules.MaxBytesPerFile, null,
            new Progress<ContentScanProgress>(p => { lock (reports) reports.Add(p); }),
            CancellationToken.None);

        // Progress<T> marshals, so give the callback a moment to land.
        SpinWait.SpinUntil(() => { lock (reports) return reports.Count > 0; }, TimeSpan.FromSeconds(2));
        lock (reports) Assert.NotEmpty(reports);
    }
}
