using System.Text;

namespace BertBrowser.Core.Ipc;

/// <summary>
/// The framing both of this app's pipes use: one UTF-8 line per message, <c>\n</c>-terminated,
/// bounded.
/// </summary>
/// <remarks>
/// <para>
/// Shared rather than duplicated because the bound is the point. A peer that connects and then
/// streams without ever sending a newline must be cut off rather than allowed to grow a buffer
/// until the process dies, and that rule is worth having in exactly one place — the single-instance
/// pipe and the index-helper pipe both face a peer they did not write.
/// </para>
/// <para>
/// Reading is a <see cref="LineReader"/> rather than a static call, and that is load-bearing for
/// the index protocol. A read fills a buffer, and whatever arrived after the newline belongs to the
/// <em>next</em> message; a static read-one-line has nowhere to keep it and silently drops it. The
/// single-instance pipe never noticed because it reads exactly one line per connection, but the
/// index helper pushes many down one stream, and there the lost bytes are lost messages.
/// </para>
/// </remarks>
public static class LineChannel
{
    /// <summary>Writes one line and flushes, so the peer sees it without waiting on a buffer.</summary>
    public static void WriteLine(Stream stream, string line)
    {
        var payload = Encoding.UTF8.GetBytes(line + "\n");
        stream.Write(payload, 0, payload.Length);
        stream.Flush();
    }

    /// <summary>Reads a single bounded line and discards anything buffered after it. Only correct
    /// where the peer sends one message per stream, as the single-instance pipe does.</summary>
    public static string? ReadLine(Stream stream, int maxLength) =>
        new LineReader(stream, maxLength).ReadLine();
}

/// <summary>
/// Reads <c>\n</c>-terminated UTF-8 lines from a stream, keeping whatever it buffered past the end
/// of one line for the next.
/// </summary>
/// <remarks>
/// The newline scan is byte-wise and safe for multi-byte text without decoding as it goes: every
/// continuation byte of a UTF-8 sequence has its high bit set, so <c>0x0A</c> can only ever appear
/// as a real newline and never inside a character. The accumulated bytes are decoded once, at the
/// end of the line, so a character split across two reads still lands whole.
/// </remarks>
public sealed class LineReader
{
    private readonly Stream _stream;
    private readonly int _maxLength;
    private readonly byte[] _buffer = new byte[1024];
    private int _start;
    private int _end;

    public LineReader(Stream stream, int maxLength)
    {
        _stream = stream;
        _maxLength = maxLength;
    }

    /// <summary>
    /// The next line, or null at end of stream with nothing buffered. A stream that ends without a
    /// newline yields what arrived, since that may still be a whole message. A peer that never
    /// sends one is cut off at the cap.
    /// </summary>
    public string? ReadLine()
    {
        var line = new MemoryStream();

        while (true)
        {
            for (; _start < _end; _start++)
            {
                if (_buffer[_start] == (byte)'\n')
                {
                    _start++;
                    return Encoding.UTF8.GetString(line.ToArray());
                }

                // Cap enforced per byte, not per read: otherwise a single 1 KB fill could carry the
                // buffer well past the limit before anything checked it.
                if (line.Length >= _maxLength)
                    return Encoding.UTF8.GetString(line.ToArray());

                line.WriteByte(_buffer[_start]);
            }

            _start = 0;
            _end = _stream.Read(_buffer, 0, _buffer.Length);
            if (_end <= 0)
            {
                _end = 0;
                return line.Length > 0 ? Encoding.UTF8.GetString(line.ToArray()) : null;
            }
        }
    }
}
