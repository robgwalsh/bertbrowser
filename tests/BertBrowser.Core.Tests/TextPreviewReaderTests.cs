using System.Text;
using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class TextPreviewReaderTests
{
    private static TextPreview Read(byte[] bytes, long budget = 1 << 20, int maxLines = 5000) =>
        TextPreviewReader.Read(new MemoryStream(bytes), budget, maxLines);

    private static TextPreview Read(string text, Encoding encoding, bool bom = false, long budget = 1 << 20)
    {
        byte[] preamble = bom ? encoding.GetPreamble() : [];
        byte[] all = [.. preamble, .. encoding.GetBytes(text)];
        return Read(all, budget);
    }

    // --- encodings ---

    [Fact]
    public void PlainAsciiIsUtf8()
    {
        var preview = Read("hello world"u8.ToArray());
        Assert.Equal("hello world", preview.Text);
        Assert.Equal("UTF-8", preview.EncodingName);
        Assert.False(preview.LooksBinary);
    }

    [Fact]
    public void Utf8WithABomIsReportedAsSuch_AndTheBomIsNotInTheText()
    {
        var preview = Read("héllo", Encoding.UTF8, bom: true);
        Assert.Equal("héllo", preview.Text);
        Assert.Equal("UTF-8 (BOM)", preview.EncodingName);
    }

    [Fact]
    public void Utf8WithoutABomIsStillDecodedAsUtf8()
    {
        var preview = Read("naïve café — ok", new UTF8Encoding(false));
        Assert.Equal("naïve café — ok", preview.Text);
        Assert.Equal("UTF-8", preview.EncodingName);
    }

    [Fact]
    public void Utf16LittleEndianWithABom()
    {
        var preview = Read("hello", Encoding.Unicode, bom: true);
        Assert.Equal("hello", preview.Text);
        Assert.Equal("UTF-16 LE", preview.EncodingName);
    }

    [Fact]
    public void Utf16BigEndianWithABom()
    {
        var preview = Read("hello", Encoding.BigEndianUnicode, bom: true);
        Assert.Equal("hello", preview.Text);
        Assert.Equal("UTF-16 BE", preview.EncodingName);
    }

    [Fact]
    public void Utf16WithoutABomIsRecognisedRatherThanCalledBinary()
    {
        // This is the case that matters: half the bytes are NUL, and a naive binary check would
        // throw away a perfectly ordinary Windows text file.
        var preview = Read("a fairly ordinary line of text", Encoding.Unicode);
        Assert.Equal("UTF-16 LE", preview.EncodingName);
        Assert.Equal("a fairly ordinary line of text", preview.Text);
        Assert.False(preview.LooksBinary);
    }

    [Fact]
    public void Utf16BigEndianWithoutABomIsRecognisedToo()
    {
        var preview = Read("a fairly ordinary line of text", Encoding.BigEndianUnicode);
        Assert.Equal("UTF-16 BE", preview.EncodingName);
        Assert.Equal("a fairly ordinary line of text", preview.Text);
    }

    [Fact]
    public void InvalidUtf8FallsBackToLatin1RatherThanLosingBytes()
    {
        // 0xE9 alone is "é" in Latin-1 and an illegal lead byte in UTF-8.
        var preview = Read([(byte)'c', (byte)'a', (byte)'f', 0xE9]);
        Assert.Equal("Latin-1", preview.EncodingName);
        Assert.Equal("café", preview.Text);
    }

    [Fact]
    public void Utf32IsRecognisedFromItsBom()
    {
        var preview = Read("hello", new UTF32Encoding(false, true), bom: true);
        Assert.Equal("UTF-32 LE", preview.EncodingName);
        Assert.Equal("hello", preview.Text);
    }

    // --- binary ---

    [Fact]
    public void ANulByteMakesItBinary()
    {
        var preview = Read([(byte)'M', (byte)'Z', 0x00, 0x03, 0x04, (byte)'x']);
        Assert.True(preview.LooksBinary);
        Assert.Equal("", preview.Text);
    }

    [Fact]
    public void BinaryIsDetectedEvenWhenTheNulIsLate()
    {
        var bytes = new byte[4096];
        Array.Fill(bytes, (byte)'a');
        bytes[4000] = 0;
        Assert.True(Read(bytes).LooksBinary);
    }

    [Fact]
    public void AnEmptyFileIsNotBinary()
    {
        var preview = Read([]);
        Assert.False(preview.LooksBinary);
        Assert.Equal("", preview.Text);
        Assert.Equal(0, preview.LineCount);
    }

    // --- line endings and counts ---

    [Theory]
    [InlineData("a\r\nb\r\nc", "CRLF")]
    [InlineData("a\nb\nc", "LF")]
    [InlineData("a\rb\rc", "CR")]
    [InlineData("a\r\nb\nc", "Mixed")]
    [InlineData("just one line", "")]
    public void TheLineEndingStyleIsReported(string text, string expected) =>
        Assert.Equal(expected, Read(text, new UTF8Encoding(false)).LineEnding);

    [Fact]
    public void EveryLineEndingIsNormalisedToNewline() =>
        Assert.Equal("a\nb\nc", Read("a\r\nb\rc", new UTF8Encoding(false)).Text);

    [Theory]
    [InlineData("a\r\nb\r\nc", 3)]
    [InlineData("a\nb\nc\n", 3)]
    [InlineData("one", 1)]
    [InlineData("", 0)]
    public void LinesAreCountedTheWayAnEditorWould(string text, int expected) =>
        // A trailing newline ends the last line; it does not begin an empty one.
        Assert.Equal(expected, Read(text, new UTF8Encoding(false)).LineCount);

    // --- truncation ---

    [Fact]
    public void TheByteBudgetTruncates_AndSaysSo()
    {
        var preview = Read(Encoding.ASCII.GetBytes(new string('x', 5000)), budget: 100);
        Assert.True(preview.Truncated);
        Assert.Equal(100, preview.Text.Length);
    }

    [Fact]
    public void AFileThatFitsIsNotReportedAsTruncated() =>
        Assert.False(Read("short"u8.ToArray(), budget: 100).Truncated);

    [Fact]
    public void AFileExactlyTheBudgetIsNotReportedAsTruncated() =>
        Assert.False(Read("12345"u8.ToArray(), budget: 5).Truncated);

    [Fact]
    public void TheLineCapTruncates_AndSaysSo()
    {
        var text = string.Join("\n", Enumerable.Range(0, 200).Select(i => $"line {i}"));
        var preview = Read(Encoding.ASCII.GetBytes(text), maxLines: 10);
        Assert.True(preview.Truncated);
        Assert.Equal(10, preview.LineCount);
        Assert.StartsWith("line 0\n", preview.Text);
        Assert.DoesNotContain("line 10", preview.Text);
    }

    [Fact]
    public void ATruncatedUtf8SequenceIsDroppedRatherThanShownAsAReplacementCharacter()
    {
        // "aaa" then a 3-byte character, cut after its first byte.
        var bytes = new List<byte> { (byte)'a', (byte)'a', (byte)'a' };
        bytes.AddRange(Encoding.UTF8.GetBytes("€"));
        var preview = Read(bytes.ToArray(), budget: 4);
        Assert.True(preview.Truncated);
        Assert.Equal("aaa", preview.Text);
        Assert.DoesNotContain("�", preview.Text);
    }

    [Fact]
    public void ATruncatedUtf16PairIsDroppedRatherThanDecodedAsRubbish()
    {
        var bytes = new List<byte>(Encoding.Unicode.GetPreamble());
        bytes.AddRange(Encoding.Unicode.GetBytes("abc"));
        // BOM (2) + "ab" (4) + half of 'c'.
        var preview = Read(bytes.ToArray(), budget: 7);
        Assert.True(preview.Truncated);
        Assert.Equal("ab", preview.Text);
    }

    // --- guessing: is this worth showing when nothing said it was text? ---

    [Fact]
    public void OrdinaryTextIsConvincing() =>
        Assert.True(TextPreviewReader.IsConvincingText(Read("<?xml version=\"1.0\"?>\n<a b=\"c\"/>\n"u8.ToArray())));

    [Fact]
    public void AnEmptyFileIsConvincing() =>
        // Showing it as a blank text pane beats refusing it.
        Assert.True(TextPreviewReader.IsConvincingText(Read([])));

    [Fact]
    public void SomethingWithANulIsNotConvincing() =>
        Assert.False(TextPreviewReader.IsConvincingText(Read([(byte)'M', (byte)'Z', 0x00, (byte)'x'])));

    [Fact]
    public void BinaryWithNoNulIsStillNotConvincing()
    {
        // The case LooksBinary alone misses: no NUL in the first 8 KB, but decoded as Latin-1 it
        // is a wall of control characters.
        var bytes = new byte[2048];
        for (var i = 0; i < bytes.Length; i++) bytes[i] = (byte)(1 + i % 0x1F);

        var preview = Read(bytes);
        Assert.False(preview.LooksBinary);                            // the loose check passes it
        Assert.False(TextPreviewReader.IsConvincingText(preview));    // the strict one does not
    }

    [Fact]
    public void TabsAndNewlinesDoNotCountAgainstIt()
    {
        var text = string.Concat(Enumerable.Repeat("\tindented\r\n", 200));
        Assert.True(TextPreviewReader.IsConvincingText(Read(Encoding.UTF8.GetBytes(text))));
    }

    [Fact]
    public void AStrayControlCharacterIsToleratedInOtherwiseGoodText()
    {
        // A form feed in a source file must not cost the whole preview.
        var text = new string('x', 500) + "\f" + new string('y', 500);
        Assert.True(TextPreviewReader.IsConvincingText(Read(Encoding.ASCII.GetBytes(text))));
    }

    [Fact]
    public void ABudgetOfZeroReadsNothingButStillNoticesThereIsMore()
    {
        var preview = Read("something"u8.ToArray(), budget: 0);
        Assert.Equal("", preview.Text);
        Assert.True(preview.Truncated);
    }
}
