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
    private byte[] _canvas;

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
    public byte[] Next(GifSubFrame frame)
    {
        var saved = frame.Disposal == GifDisposal.Restore ? (byte[])_canvas.Clone() : null;
        Draw(frame);
        var picture = (byte[])_canvas.Clone();

        if (saved is not null) _canvas = saved;
        else if (frame.Disposal == GifDisposal.Clear) ClearRect(frame);

        return picture;
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
