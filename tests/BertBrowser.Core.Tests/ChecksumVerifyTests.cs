using BertBrowser.Core.Services.Checksums;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class ChecksumVerifyTests
{
    private const string A = "AAAAAAAA";
    private const string B = "BBBBBBBB";

    private static IReadOnlyList<ChecksumVerifyRow> Reconcile(
        IReadOnlyList<ChecksumLine> listed,
        Dictionary<string, string?>? computed = null,
        string[]? refused = null,
        string[]? present = null) =>
        ChecksumVerify.Reconcile(listed, computed ?? [], refused ?? [], present ?? []);

    private static ChecksumVerifyState StateOf(IReadOnlyList<ChecksumVerifyRow> rows, string name) =>
        rows.Single(r => r.Name == name).State;

    [Fact]
    public void AMatchingDigestIsOk()
    {
        var rows = Reconcile([new ChecksumLine("a.bin", A)], new() { ["a.bin"] = A });

        Assert.Equal(ChecksumVerifyState.Ok, Assert.Single(rows).State);
    }

    [Fact]
    public void ADifferentDigestIsAMismatch_AndBothValuesAreReported()
    {
        var row = Assert.Single(Reconcile([new ChecksumLine("a.bin", A)], new() { ["a.bin"] = B }));

        Assert.Equal(ChecksumVerifyState.Mismatch, row.State);
        Assert.Equal(A, row.Expected);
        Assert.Equal(B, row.Actual);
    }

    [Fact]
    public void ALineWithNoFileIsMissing() =>
        Assert.Equal(ChecksumVerifyState.Missing, Assert.Single(Reconcile([new ChecksumLine("a.bin", A)])).State);

    /// <summary>
    /// A file that is there and could not be read is not a file that is wrong. Collapsing the two
    /// would report a locked file as corruption.
    /// </summary>
    [Fact]
    public void APresentButUnreadableFileIsUnreadable_NotAMismatch()
    {
        var rows = Reconcile([new ChecksumLine("a.bin", A)], new() { ["a.bin"] = null });

        Assert.Equal(ChecksumVerifyState.Unreadable, Assert.Single(rows).State);
    }

    [Fact]
    public void AnEscapingNameIsRefused_AndIsNeverRead()
    {
        var rows = Reconcile([new ChecksumLine(@"..\..\evil", A)], refused: [@"..\..\evil"]);

        Assert.Equal(ChecksumVerifyState.Refused, Assert.Single(rows).State);
    }

    [Fact]
    public void AFileNothingListedIsNotListed()
    {
        var rows = Reconcile([new ChecksumLine("a.bin", A)], new() { ["a.bin"] = A }, present: ["a.bin", "extra.bin"]);

        Assert.Equal(ChecksumVerifyState.Ok, StateOf(rows, "a.bin"));
        Assert.Equal(ChecksumVerifyState.NotListed, StateOf(rows, "extra.bin"));
    }

    /// <summary>
    /// A folder holding something the checksum file never mentioned is normal — the file covers
    /// what it covers. Calling that a failure would make every download folder fail.
    /// </summary>
    [Fact]
    public void NotListedIsNotAFailure()
    {
        Assert.False(ChecksumVerify.IsFailure(ChecksumVerifyState.NotListed));
        Assert.False(ChecksumVerify.IsFailure(ChecksumVerifyState.Ok));

        Assert.True(ChecksumVerify.IsFailure(ChecksumVerifyState.Mismatch));
        Assert.True(ChecksumVerify.IsFailure(ChecksumVerifyState.Missing));
        Assert.True(ChecksumVerify.IsFailure(ChecksumVerifyState.Unreadable));
        Assert.True(ChecksumVerify.IsFailure(ChecksumVerifyState.Refused));
    }

    [Fact]
    public void NamesMatchCaseInsensitively_BecauseWindows()
    {
        var rows = Reconcile([new ChecksumLine("Setup.exe", A)], new() { ["Setup.exe"] = A }, present: ["SETUP.EXE"]);

        Assert.Equal(ChecksumVerifyState.Ok, Assert.Single(rows).State);
    }

    [Fact]
    public void DigestsMatchCaseInsensitively_BecausePublishersDisagree()
    {
        var rows = Reconcile([new ChecksumLine("a.bin", "cbf43926")], new() { ["a.bin"] = "CBF43926" });

        Assert.Equal(ChecksumVerifyState.Ok, Assert.Single(rows).State);
    }

    [Fact]
    public void EveryListedLineGetsExactlyOneRow()
    {
        var rows = Reconcile(
            [new ChecksumLine("a.bin", A), new ChecksumLine("b.bin", A), new ChecksumLine("c.bin", A)],
            new() { ["a.bin"] = A, ["b.bin"] = B });

        Assert.Equal(3, rows.Count);
        Assert.Equal(ChecksumVerifyState.Ok, StateOf(rows, "a.bin"));
        Assert.Equal(ChecksumVerifyState.Mismatch, StateOf(rows, "b.bin"));
        Assert.Equal(ChecksumVerifyState.Missing, StateOf(rows, "c.bin"));
    }

    [Fact]
    public void TheSummaryLeadsWithWhatWentWrong()
    {
        var rows = Reconcile(
            [new ChecksumLine("a.bin", A), new ChecksumLine("b.bin", A)],
            new() { ["a.bin"] = A, ["b.bin"] = B });

        Assert.StartsWith("1 mismatched", ChecksumVerify.Summarise(rows), StringComparison.Ordinal);
    }

    [Fact]
    public void AnEmptyReportSaysSo() => Assert.Equal("Nothing to verify.", ChecksumVerify.Summarise([]));
}
