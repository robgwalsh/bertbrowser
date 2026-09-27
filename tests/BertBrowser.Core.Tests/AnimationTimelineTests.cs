using BertBrowser.Core.Services.Preview.Animation;
using Xunit;

namespace BertBrowser.Core.Tests;

public class AnimationTimelineTests
{
    [Fact]
    public void ADelayTooShortToHonourPlaysAtTheBrowserDefault()
    {
        var timeline = new AnimationTimeline([0, 10, 20]);
        Assert.Equal([100, 100, 20], timeline.Delays);
        Assert.Equal(220, timeline.Duration);
    }

    [Theory]
    [InlineData(0, 0)]
    [InlineData(99, 0)]
    [InlineData(100, 1)]
    [InlineData(349, 1)]
    [InlineData(350, 2)]
    [InlineData(399, 2)]
    public void APositionLandsOnTheFrameShowingThen(long position, int frame)
    {
        var timeline = new AnimationTimeline([100, 250, 50]);
        Assert.Equal(frame, timeline.FrameAt(position));
    }

    [Fact]
    public void PastTheEndWrapsToTheStart()
    {
        var timeline = new AnimationTimeline([100, 250, 50]);
        Assert.Equal(0, timeline.FrameAt(400));
        Assert.Equal(1, timeline.FrameAt(400 * 7 + 150));
        Assert.Equal(2, timeline.FrameAt(-1));
    }

    [Fact]
    public void TheNextChangeIsTheEndOfTheCurrentFrame()
    {
        var timeline = new AnimationTimeline([100, 250, 50]);
        Assert.Equal(100, timeline.UntilNextFrame(0));
        Assert.Equal(40, timeline.UntilNextFrame(310));
        Assert.Equal(10, timeline.UntilNextFrame(390));
        Assert.Equal(100, timeline.UntilNextFrame(400));
    }

    [Fact]
    public void StartOfIsClampedToTheFramesThatExist()
    {
        var timeline = new AnimationTimeline([100, 250, 50]);
        Assert.Equal(350, timeline.StartOf(2));
        Assert.Equal(350, timeline.StartOf(99));
        Assert.Equal(0, timeline.StartOf(-1));
    }

    [Fact]
    public void ASingleFrameIsItsOwnWholeLoop()
    {
        var timeline = new AnimationTimeline([0]);
        Assert.Equal(1, timeline.FrameCount);
        Assert.Equal(0, timeline.FrameAt(12345));
        Assert.True(timeline.UntilNextFrame(12345) >= 1);
    }

    [Theory]
    [InlineData(1, 10, 10, false)]      // a still picture is not an animation
    [InlineData(2, 10, 10, true)]
    [InlineData(AnimationLimits.MaxFrames + 1, 1, 1, false)]
    [InlineData(100, 1000, 1000, false)] // 400 MB decoded
    [InlineData(2, 0, 10, false)]
    public void TheLimitsAreAskedOfTheHeader(int frames, int width, int height, bool allowed) =>
        Assert.Equal(allowed, AnimationLimits.Allows(frames, width, height));
}
