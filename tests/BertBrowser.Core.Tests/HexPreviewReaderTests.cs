using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class HexPreviewReaderTests
{
    private static HexPreview Read(byte[] bytes, long budget = 1 << 20, int maxRows = 5000) =>
        HexPreviewReader.Read(new MemoryStream(bytes), budget, maxRows);

    private static byte[] Bytes(int count)
    {
        var bytes = new byte[count];
        for (var i = 0; i < count; i++) bytes[i] = (byte)i;
        return bytes;
    }

    // --- the row ---

    [Fact]
    public void TheFirstBytesOfAnExecutableLookLikeAnExecutable()
    {
        var preview = Read([0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00]);
        var row = Assert.Single(preview.Rows);
        Assert.StartsWith("00000000  4D 5A 90 00 03 00 00 00 ", row.Text);
        Assert.EndsWith("MZ......        ", row.Text);
    }

    [Fact]
    public void OffsetsAdvanceBySixteenAndAreEightHexDigits()
    {
        var rows = Read(Bytes(40)).Rows;
        Assert.Equal([0, 16, 32], rows.Select(r => r.Offset));
        Assert.Equal(["00000000", "00000010", "00000020"], rows.Select(r => r.Text[..8]));
    }

    [Fact]
    public void AShortFinalRowPadsBothColumns()
    {
        // Otherwise the ASCII gutter slides left on the last line of every file whose length is
        // not a multiple of sixteen — which is nearly all of them.
        var rows = Read(Bytes(20)).Rows;
        Assert.Equal(rows[0].Text.Length, rows[1].Text.Length);
    }

    [Theory]
    [InlineData((byte)0x20, ' ')]   // space is printable, and a run of them is meaningful
    [InlineData((byte)0x41, 'A')]
    [InlineData((byte)0x7E, '~')]
    [InlineData((byte)0x00, '.')]
    [InlineData((byte)0x1F, '.')]
    [InlineData((byte)0x7F, '.')]   // DEL is not printable however tempting its position is
    [InlineData((byte)0xFF, '.')]
    public void TheAsciiColumnShowsWhatCanBeShownAndADotForTheRest(byte value, char shown)
    {
        var row = Assert.Single(Read([value]).Rows);
        Assert.Equal(shown, row.Text[^HexPreviewReader.BytesPerRow]);
    }

    // --- the cover, which is the one that must never break ---

    [Theory]
    [InlineData(1)]
    [InlineData(8)]
    [InlineData(16)]
    [InlineData(17)]
    [InlineData(255)]
    public void SpansCoverEveryRowExactlyOnce(int length)
    {
        // The view builds one run per span and renders nothing else, so a gap here does not show
        // as uncoloured text — it deletes the characters underneath it.
        foreach (var row in Read(Bytes(length)).Rows)
        {
            var covered = 0;
            foreach (var span in row.Spans)
            {
                Assert.True(span.Length > 0, "an empty span reached the view");
                Assert.Equal(covered, span.Start);
                covered += span.Length;
            }
            Assert.Equal(row.Text.Length, covered);
        }
    }

    // --- how much is read ---

    [Fact]
    public void AFileExactlyTheBudgetIsNotReportedAsTruncated()
    {
        var preview = Read(Bytes(64), budget: 64);
        Assert.False(preview.Truncated);
        Assert.Equal(64, preview.BytesShown);
    }

    [Fact]
    public void OneByteMoreThanTheBudgetIsTruncated()
    {
        var preview = Read(Bytes(65), budget: 64);
        Assert.True(preview.Truncated);
        Assert.Equal(64, preview.BytesShown);
    }

    [Fact]
    public void TheRowCapBoundsTheReadWhateverBudgetItIsGiven()
    {
        // The point of the cap: a megabyte of hex is 65,536 paragraphs, so the budget alone is not
        // a ceiling on the work the view does.
        var preview = Read(Bytes(200), budget: 1 << 20, maxRows: 4);
        Assert.Equal(4, preview.Rows.Count);
        Assert.Equal(64, preview.BytesShown);
        Assert.True(preview.Truncated);
    }

    [Fact]
    public void AnEmptyFileHasNoRows() =>
        Assert.Empty(Read([]).Rows);

    [Fact]
    public void ABudgetOfZeroReadsNothingButStillNoticesThereIsMore()
    {
        var preview = Read(Bytes(16), budget: 0);
        Assert.Empty(preview.Rows);
        Assert.True(preview.Truncated);
    }

    // --- the meta-test: the cover check above can actually fail ---

    [Fact]
    public void TheCoverCheckNoticesAGap()
    {
        IReadOnlyList<SyntaxSpan> holed =
            [new SyntaxSpan(0, 4, SyntaxClass.Comment), new SyntaxSpan(5, 3, SyntaxClass.Number)];

        var covered = 0;
        var contiguous = true;
        foreach (var span in holed)
        {
            if (span.Start != covered) contiguous = false;
            covered += span.Length;
        }
        Assert.False(contiguous);
    }
}
