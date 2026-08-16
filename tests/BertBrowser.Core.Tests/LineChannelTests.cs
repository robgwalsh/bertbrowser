using System.Text;
using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The framing under both of this app's pipes. The bound is the part worth testing: a peer that
/// never sends a newline must be cut off rather than allowed to grow the buffer forever.
/// </summary>
public class LineChannelTests
{
    private const int Max = 64 * 1024;

    private static string? Read(byte[] bytes, int maxLength = Max) =>
        LineChannel.ReadLine(new MemoryStream(bytes), maxLength);

    private static string? Read(string text, int maxLength = Max) =>
        Read(Encoding.UTF8.GetBytes(text), maxLength);

    [Fact]
    public void ReadsOneLine()
    {
        Assert.Equal("OPEN\tDefault\t-C:\\Temp", Read("OPEN\tDefault\t-C:\\Temp\n"));
    }

    /// <summary>
    /// The one that caught the bug worth catching: a static read-one-line drops whatever the same
    /// buffer fill carried past the newline, so the second message never arrives.
    /// </summary>
    [Fact]
    public void ConsecutiveLinesFromOneBufferFillAllArrive()
    {
        var reader = new LineReader(new MemoryStream(Encoding.UTF8.GetBytes("first\nsecond\nthird\n")), Max);

        Assert.Equal("first", reader.ReadLine());
        Assert.Equal("second", reader.ReadLine());
        Assert.Equal("third", reader.ReadLine());
        Assert.Null(reader.ReadLine());
    }

    /// <summary>Many small messages, more than one buffer fill's worth.</summary>
    [Fact]
    public void ManyLinesAcrossManyBufferFillsAllArrive()
    {
        var lines = Enumerable.Range(0, 500).Select(i => $"IDX\tComplete\tC:\\LINE{i}").ToList();
        var stream = new MemoryStream(Encoding.UTF8.GetBytes(string.Join("\n", lines) + "\n"));
        var reader = new LineReader(stream, Max);

        foreach (var expected in lines)
            Assert.Equal(expected, reader.ReadLine());

        Assert.Null(reader.ReadLine());
    }

    /// <summary>A stream that ends without a newline may still have delivered a whole message.</summary>
    [Fact]
    public void AcceptsALineWithNoTrailingNewline()
    {
        Assert.Equal("OPEN\tDefault", Read("OPEN\tDefault"));
    }

    [Fact]
    public void ReturnsNullOnAnEmptyStream()
    {
        Assert.Null(Read(""));
    }

    [Fact]
    public void ReturnsEmptyForABareNewline()
    {
        Assert.Equal("", Read("\n"));
    }

    /// <summary>The internal read buffer is 1 KB, so this line spans several of them.</summary>
    [Fact]
    public void ReadsALineSpanningManyBufferFills()
    {
        var line = new string('a', 5000);

        Assert.Equal(line, Read(line + "\n"));
    }

    /// <summary>
    /// Decoding happens once over the accumulated bytes, so a character whose bytes land in
    /// different reads still comes back whole. Decode-as-you-go would mangle this.
    /// </summary>
    [Fact]
    public void ReadsMultiByteCharactersSplitAcrossReads()
    {
        // 400 three-byte characters straddles the 1 KB buffer boundary mid-character.
        var line = new string('…', 400);

        Assert.Equal(line, Read(line + "\n"));
    }

    /// <summary>
    /// A newline byte can never appear inside a UTF-8 sequence — continuation bytes all have the
    /// high bit set — so scanning bytes for 0x0A cannot split a character.
    /// </summary>
    [Fact]
    public void MultiByteCharactersDoNotProduceFalseNewlines()
    {
        var line = "路徑\u00e9\u20ac\U0001F600";

        Assert.Equal(line, Read(line + "\n"));
    }

    /// <summary>
    /// The cap is per byte, not per buffer fill — a single 1 KB read must not be able to carry the
    /// line well past the limit before anything checks it.
    /// </summary>
    [Fact]
    public void CutsOffAPeerThatNeverSendsANewline()
    {
        var result = Read(new string('a', 900), maxLength: 64);

        Assert.Equal(64, result!.Length);
    }

    [Fact]
    public void CutsOffAnEndlessStreamAtTheCap()
    {
        var result = LineChannel.ReadLine(new EndlessStream(), maxLength: 4096);

        Assert.Equal(4096, result!.Length);
    }

    [Fact]
    public void WriteLineTerminatesWithANewlineInUtf8()
    {
        var stream = new MemoryStream();

        LineChannel.WriteLine(stream, "IDX\tComplete\tC:\\");

        Assert.Equal("IDX\tComplete\tC:\\\n", Encoding.UTF8.GetString(stream.ToArray()));
    }

    [Fact]
    public void WrittenLinesRoundTrip()
    {
        var stream = new MemoryStream();
        LineChannel.WriteLine(stream, "one");
        LineChannel.WriteLine(stream, "two…");
        stream.Position = 0;

        var reader = new LineReader(stream, Max);
        Assert.Equal("one", reader.ReadLine());
        Assert.Equal("two…", reader.ReadLine());
        Assert.Null(reader.ReadLine());
    }

    /// <summary>A peer that writes forever and never terminates a line.</summary>
    private sealed class EndlessStream : Stream
    {
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => 0; set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

        public override int Read(byte[] buffer, int offset, int count)
        {
            for (var i = 0; i < count; i++) buffer[offset + i] = (byte)'a';
            return count;
        }
    }
}
