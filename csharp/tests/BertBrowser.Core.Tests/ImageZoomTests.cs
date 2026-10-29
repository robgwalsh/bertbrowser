using BertBrowser.Core.Services.Preview;
using Xunit;

namespace BertBrowser.Core.Tests;

public class ImageZoomTests
{
    [Fact]
    public void ANotchInAndANotchOutComeBackToWhereTheyStarted()
    {
        var zoomed = ImageZoom.Step(2, 1);
        Assert.Equal(2 * ImageZoom.StepFactor, zoomed, 9);
        Assert.Equal(2, ImageZoom.Step(zoomed, -1), 9);
    }

    [Fact]
    public void ZoomingOutStopsAtFit()
    {
        Assert.Equal(1, ImageZoom.Step(1.1, -5));
    }

    [Fact]
    public void ZoomingOutMayGoBelowFitWhenOneToOneIsSmaller()
    {
        // A small image shown at 100% is smaller than fitted; the wheel must not snap it back up.
        Assert.Equal(0.5, ImageZoom.Clamp(0.5, minScale: 0.5));
        Assert.Equal(0.5, ImageZoom.Step(0.5, -3, minScale: 0.5));
    }

    [Fact]
    public void ZoomingInStopsAtTheCeiling()
    {
        Assert.Equal(ImageZoom.MaxScale, ImageZoom.Step(ImageZoom.MaxScale, 3));
    }

    [Fact]
    public void ThePointUnderTheCursorStaysPut()
    {
        // Image point 100 drawn at 100 * 2 + 10 = 210; after zooming to 3 it must still land at 210.
        var offset = ImageZoom.ZoomOffset(10, anchor: 100, oldScale: 2, newScale: 3);
        Assert.Equal(210, 100 * 3 + offset);
    }

    [Fact]
    public void AnImageSmallerThanThePaneIsCentred()
    {
        // Fitted 200 wide at origin 50 in a 300 pane; at scale 1 it belongs where layout put it.
        Assert.Equal(0, ImageZoom.ClampOffset(80, origin: 50, length: 200, scale: 1, viewport: 300));
    }

    [Fact]
    public void AZoomedImageCannotBeDraggedPastItsEdges()
    {
        // 200 wide at scale 2 = 400 in a 300 pane, origin 50: left edge at most 0, right edge at least 300.
        Assert.Equal(-50, ImageZoom.ClampOffset(500, origin: 50, length: 200, scale: 2, viewport: 300));
        Assert.Equal(-150, ImageZoom.ClampOffset(-500, origin: 50, length: 200, scale: 2, viewport: 300));
        Assert.Equal(-100, ImageZoom.ClampOffset(-100, origin: 50, length: 200, scale: 2, viewport: 300));
    }
}
