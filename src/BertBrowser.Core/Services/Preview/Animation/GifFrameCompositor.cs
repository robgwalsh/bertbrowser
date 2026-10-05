namespace BertBrowser.Core.Services.Preview.Animation;

/// <summary>What a GIF frame asks to be done with its area once its delay is up.</summary>
public enum GifDisposal
{
    /// <summary>0 and 1 in the file: leave it where it is for the next frame to draw over.</summary>
    Keep = 1,

    /// <summary>2: clear its rectangle. To transparent, as every browser does, not to the
    /// "background colour" the spec names and nobody honours.</summary>
    Clear = 2,

    /// <summary>3: put back whatever was there before it was drawn.</summary>
    Restore = 3,
}

/// <summary>One frame as the file stores it: a rectangle of BGRA32 pixels somewhere on the canvas.</summary>
public sealed record GifSubFrame(int Left, int Top, int Width, int Height, byte[] Pixels, GifDisposal Disposal);

/// <summary>
/// Turns a GIF's frames — each only the rectangle that changed — into whole pictures, one per
/// frame, so that showing any frame is a lookup rather than a replay from the first.
/// </summary>
/// <remarks>
/// Fed one frame at a time (<see cref="Next"/>) so the preview can show the first frame, and start
/// playing, while the rest are still being decoded. Pure byte arithmetic with no imaging
/// dependency, so the disposal rules are tested here rather than trusted to a decoder. A GIF is
/// untrusted input: a rectangle that runs off the canvas is clipped, and a pixel buffer shorter
/// than its rectangle claims is drawn as far as it goes. A GIF pixel is fully opaque or fully
/// transparent, so drawing is a copy wherever alpha is set.
/// </remarks>
public sealed class GifFrameCompositor
{
    private readonly int _width;
    private readonly int _height;
    private readonly byte[] _canvas;

    /// <summary>The frame last drawn, whose disposal is still owed — carried out at the start of
    /// the next call, so the canvas stays that frame's picture for as long as a caller may be
    /// reading it.</summary>
    private GifSubFrame? _undisposed;

    /// <summary>What was under a <see cref="GifDisposal.Restore"/> frame's rectangle. Only the
    /// rectangle, and one buffer reused: saving the whole canvas per frame was a large-object
    /// allocation for every frame of the animation.</summary>
    private byte[] _under = [];

    public GifFrameCompositor(int width, int height)
    {
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(width);
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(height);
        _width = width;
        _height = height;
        _canvas = new byte[checked(width * height * 4)];
    }

    /// <summary>Draws the next frame and returns the whole picture it makes — a copy of its own,
    /// since the canvas goes on to be disposed of and drawn over.</summary>
    public byte[] Next(GifSubFrame frame) => (byte[])NextShared(frame).Clone();

    /// <summary>
    /// Draws the next frame and returns the canvas itself, which is that frame's whole picture
    /// <b>only until the next call</b>. For a caller that copies the pixels somewhere of its own
    /// straight away — a bitmap does — and so has no use for a second copy in between.
    /// </summary>
    public byte[] NextShared(GifSubFrame frame)
    {
        if (_undisposed is { } previous)
        {
            if (previous.Disposal == GifDisposal.Restore) CopyRect(previous, restore: true);
            else if (previous.Disposal == GifDisposal.Clear) ClearRect(previous);
        }

        if (frame.Disposal == GifDisposal.Restore) CopyRect(frame, restore: false);
        Draw(frame);
        _undisposed = frame;
        return _canvas;
    }

    /// <summary>Saves the frame's clipped rectangle out of the canvas, or puts it back.</summary>
    private void CopyRect(GifSubFrame frame, bool restore)
    {
        var (x0, y0, x1, y1) = Clip(frame);
        var rowBytes = (x1 - x0) * 4;
        var needed = rowBytes * (y1 - y0);
        if (!restore && _under.Length < needed) _under = new byte[needed];

        for (var y = y0; y < y1; y++)
        {
            var canvasAt = (y * _width + x0) * 4;
            var underAt = (y - y0) * rowBytes;
            if (restore) Buffer.BlockCopy(_under, underAt, _canvas, canvasAt, rowBytes);
            else Buffer.BlockCopy(_canvas, canvasAt, _under, underAt, rowBytes);
        }
    }

    /// <summary>Every frame at once.</summary>
    public static IReadOnlyList<byte[]> Compose(int width, int height, IEnumerable<GifSubFrame> frames, CancellationToken ct = default)
    {
        var compositor = new GifFrameCompositor(width, height);
        var composed = new List<byte[]>();
        foreach (var frame in frames)
        {
            ct.ThrowIfCancellationRequested();
            composed.Add(compositor.Next(frame));
        }
        return composed;
    }

    private void Draw(GifSubFrame frame)
    {
        var (x0, y0, x1, y1) = Clip(frame);
        for (var y = y0; y < y1; y++)
        {
            for (var x = x0; x < x1; x++)
            {
                var src = ((y - frame.Top) * frame.Width + (x - frame.Left)) * 4;
                if (src + 3 >= frame.Pixels.Length) return;
                if (frame.Pixels[src + 3] == 0) continue;

                var dst = (y * _width + x) * 4;
                _canvas[dst] = frame.Pixels[src];
                _canvas[dst + 1] = frame.Pixels[src + 1];
                _canvas[dst + 2] = frame.Pixels[src + 2];
                _canvas[dst + 3] = frame.Pixels[src + 3];
            }
        }
    }

    private void ClearRect(GifSubFrame frame)
    {
        var (x0, y0, x1, y1) = Clip(frame);
        for (var y = y0; y < y1; y++)
            Array.Clear(_canvas, (y * _width + x0) * 4, (x1 - x0) * 4);
    }

    private (int X0, int Y0, int X1, int Y1) Clip(GifSubFrame frame)
    {
        var x0 = Math.Clamp(frame.Left, 0, _width);
        var y0 = Math.Clamp(frame.Top, 0, _height);
        var x1 = (int)Math.Clamp((long)frame.Left + Math.Max(0, frame.Width), x0, _width);
        var y1 = (int)Math.Clamp((long)frame.Top + Math.Max(0, frame.Height), y0, _height);
        return (x0, y0, x1, y1);
    }
}
