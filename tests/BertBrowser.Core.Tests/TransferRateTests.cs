using BertBrowser.Core.Services.Transfer;
using Xunit;

namespace BertBrowser.Core.Tests;

public class TransferRateTests
{
    private static TimeSpan At(double seconds) => TimeSpan.FromSeconds(seconds);

    private static TransferEstimate Known(long bytes) => new(bytes, 1, Complete: true);

    [Fact]
    public void OneSample_IsNotEnoughToNameARate()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));

        Assert.Null(rate.BytesPerSecond);
    }

    [Fact]
    public void TwoSamples_GiveTheThroughputBetweenThem()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1_000_000, At(1));

        Assert.Equal(1_000_000d, rate.BytesPerSecond!.Value, 1d);
    }

    [Fact]
    public void ASampleTooCloseToTheLast_IsHeldRatherThanThrownAway()
    {
        // A fast reporter must still contribute its bytes; dropping them would under-report.
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(500_000, At(0.1));   // too soon to divide by
        Assert.Null(rate.BytesPerSecond);

        rate.Observe(1_000_000, At(1));
        Assert.Equal(1_000_000d, rate.BytesPerSecond!.Value, 1d);
    }

    [Fact]
    public void ASuddenSpike_IsSmoothedRatherThanFollowed()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1_000_000, At(1));            // 1 MB/s
        rate.Observe(1_000_000 + 100_000_000, At(2)); // one 100 MB/s second

        // Somewhere between the two, and much nearer the established figure.
        Assert.InRange(rate.BytesPerSecond!.Value, 1_000_000d, 50_000_000d);
    }

    [Fact]
    public void AStalledTransfer_DecaysTowardsNothing()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(10_000_000, At(1));
        var moving = rate.BytesPerSecond!.Value;

        for (var second = 2; second <= 12; second++)
            rate.Observe(10_000_000, At(second)); // nothing more arrives

        Assert.True(rate.BytesPerSecond!.Value < moving / 10,
            "a transfer that stopped must not keep reporting the rate it had.");
    }

    [Fact]
    public void Remaining_NeedsARate()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));

        Assert.Null(rate.Remaining(0, Known(1_000_000)));
    }

    [Fact]
    public void Remaining_DividesWhatIsLeftByTheRate()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1_000_000, At(1)); // 1 MB/s

        var left = rate.Remaining(1_000_000, Known(7_000_000));

        Assert.Equal(6d, left!.Value.TotalSeconds, 0.1d);
    }

    [Fact]
    public void Remaining_IsNothing_WhenTheTotalIsOnlyAFloor()
    {
        // The size index had no row for something in the plan. The remainder is unknown, not small,
        // and turning it into a time would be an invention.
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1_000_000, At(1));

        Assert.Null(rate.Remaining(1_000_000, new TransferEstimate(7_000_000, 1, Complete: false)));
    }

    [Fact]
    public void Remaining_IsZero_OnceEverythingIsAccountedFor()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1_000_000, At(1));

        Assert.Equal(TimeSpan.Zero, rate.Remaining(1_000_000, Known(1_000_000)));
    }

    [Fact]
    public void AnAbsurdEstimate_IsWithheldRatherThanShown()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(1, At(1)); // one byte per second

        Assert.Null(rate.Remaining(0, Known(long.MaxValue / 2)));
    }

    [Fact]
    public void ACountThatGoesBackwards_ReSeedsInsteadOfGoingNegative()
    {
        var rate = new TransferRate();
        rate.Observe(0, At(0));
        rate.Observe(10_000_000, At(1));

        rate.Observe(0, At(2));           // a second transfer reusing the instance
        rate.Observe(2_000_000, At(3));

        Assert.True(rate.BytesPerSecond > 0);
    }
}
