using BertBrowser.Core.Services.Preview.Animation;
using Xunit;

namespace BertBrowser.Core.Tests;

public class GifFrameCompositorTests
{
    // A 2×1 canvas throughout: pixel 0 on the left, pixel 1 on the right.

    private static readonly byte[] Red = [0, 0, 255, 255];
    private static readonly byte[] Blue = [255, 0, 0, 255];
    private static readonly byte[] Clear = [0, 0, 0, 0];

    private static byte[] Pixels(params byte[][] pixels) => pixels.SelectMany(p => p).ToArray();

    private static byte[] PixelAt(byte[] canvas, int index) => canvas[(index * 4)..(index * 4 + 4)];

    [Fact]
    public void AKeptFrameStaysUnderTheNext()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 2, 1, Pixels(Red, Red), GifDisposal.Keep),
            new GifSubFrame(1, 0, 1, 1, Blue, GifDisposal.Keep),
        ]);

        Assert.Equal(Red, PixelAt(frames[1], 0));
        Assert.Equal(Blue, PixelAt(frames[1], 1));
    }

    [Fact]
    public void ATransparentPixelShowsWhatIsBeneath()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 2, 1, Pixels(Red, Red), GifDisposal.Keep),
            new GifSubFrame(0, 0, 2, 1, Pixels(Clear, Blue), GifDisposal.Keep),
        ]);

        Assert.Equal(Red, PixelAt(frames[1], 0));
        Assert.Equal(Blue, PixelAt(frames[1], 1));
    }

    [Fact]
    public void AClearedFrameLeavesItsRectangleTransparent()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 1, 1, Red, GifDisposal.Keep),
            new GifSubFrame(1, 0, 1, 1, Blue, GifDisposal.Clear),
            new GifSubFrame(0, 0, 1, 1, Clear, GifDisposal.Keep),
        ]);

        Assert.Equal(Blue, PixelAt(frames[1], 1));   // shown in full first
        Assert.Equal(Red, PixelAt(frames[2], 0));    // outside the rectangle, untouched
        Assert.Equal(Clear, PixelAt(frames[2], 1));  // then cleared
    }

    [Fact]
    public void ARestoredFramePutsBackWhatWasThereBefore()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 2, 1, Pixels(Red, Red), GifDisposal.Keep),
            new GifSubFrame(0, 0, 2, 1, Pixels(Blue, Blue), GifDisposal.Restore),
            new GifSubFrame(0, 0, 1, 1, Clear, GifDisposal.Keep),
        ]);

        Assert.Equal(Blue, PixelAt(frames[1], 0));
        Assert.Equal(Red, PixelAt(frames[2], 0));
        Assert.Equal(Red, PixelAt(frames[2], 1));
    }

    [Fact]
    public void EachFrameIsItsOwnCopy()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 1, 1, Red, GifDisposal.Keep),
            new GifSubFrame(0, 0, 1, 1, Blue, GifDisposal.Keep),
        ]);

        Assert.Equal(Red, PixelAt(frames[0], 0));
        Assert.Equal(Blue, PixelAt(frames[1], 0));
    }

    [Fact]
    public void ARectangleOffTheCanvasIsClippedRatherThanThrowing()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(1, 0, 3, 2, Pixels(Blue, Blue, Blue, Blue, Blue, Blue), GifDisposal.Clear),
            new GifSubFrame(-5, -5, 1, 1, Red, GifDisposal.Keep),
            new GifSubFrame(int.MaxValue, 0, int.MaxValue, 1, Red, GifDisposal.Keep),
        ]);

        Assert.Equal(Clear, PixelAt(frames[0], 0));
        Assert.Equal(Blue, PixelAt(frames[0], 1));
        Assert.Equal(Clear, PixelAt(frames[1], 1));
        Assert.Equal(3, frames.Count);
    }

    [Fact]
    public void APixelBufferShorterThanItsRectangleIsDrawnAsFarAsItGoes()
    {
        var frames = GifFrameCompositor.Compose(2, 1,
        [
            new GifSubFrame(0, 0, 2, 1, Red, GifDisposal.Keep),
        ]);

        Assert.Equal(Red, PixelAt(frames[0], 0));
        Assert.Equal(Clear, PixelAt(frames[0], 1));
    }
}
