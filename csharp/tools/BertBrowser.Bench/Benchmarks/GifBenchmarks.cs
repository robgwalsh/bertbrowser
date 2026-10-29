using BenchmarkDotNet.Attributes;
using BertBrowser.Core.Services.Preview.Animation;

namespace BertBrowser.Bench.Benchmarks;

/// <summary>
/// Compositing sixty GIF sub-frames onto a 320×240 canvas — a full first frame, then small
/// rectangles cycling through the three disposal methods, which is what a typical animated GIF is.
/// </summary>
public class GifBenchmarks
{
    private const int Width = 320;
    private const int Height = 240;

    private GifSubFrame[] _frames = [];

    [GlobalSetup]
    public void Setup()
    {
        var rng = new Random(11);
        var frames = new List<GifSubFrame>(60) { new(0, 0, Width, Height, Opaque(Width, Height, rng), GifDisposal.Keep) };
        for (var i = 1; i < 60; i++)
        {
            var w = rng.Next(16, 120);
            var h = rng.Next(16, 90);
            var disposal = (GifDisposal)(1 + i % 3);
            frames.Add(new GifSubFrame(rng.Next(0, Width - w), rng.Next(0, Height - h), w, h, Opaque(w, h, rng, transparentEvery: 5), disposal));
        }
        _frames = frames.ToArray();
    }

    [Benchmark]
    public int Next60Frames()
    {
        var compositor = new GifFrameCompositor(Width, Height);
        var bytes = 0;
        foreach (var frame in _frames) bytes += compositor.Next(frame).Length;
        return bytes;
    }

    /// <summary>The same sixty frames the way the preview takes them: the canvas, not a copy.</summary>
    [Benchmark]
    public int NextShared60Frames()
    {
        var compositor = new GifFrameCompositor(Width, Height);
        var bytes = 0;
        foreach (var frame in _frames) bytes += compositor.NextShared(frame).Length;
        return bytes;
    }

    /// <summary>BGRA32 pixels, fully opaque except every nth, which a GIF leaves transparent.</summary>
    private static byte[] Opaque(int width, int height, Random rng, int transparentEvery = 0)
    {
        var pixels = new byte[width * height * 4];
        rng.NextBytes(pixels);
        for (var i = 3; i < pixels.Length; i += 4)
            pixels[i] = transparentEvery > 0 && (i / 4) % transparentEvery == 0 ? (byte)0 : (byte)255;
        return pixels;
    }
}
