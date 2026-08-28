namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// Turns a rising byte count into a throughput figure and a time remaining.
/// </summary>
/// <remarks>
/// <para>
/// <b>Smoothed, because the raw figure is useless.</b> Copy throughput swings wildly between
/// chunks — cache flushes, a small file among large ones, a network path catching its breath — and
/// a number recomputed from the last interval alone flickers between "8 MB/s" and "400 MB/s"
/// several times a second. An exponential moving average is what makes it readable.
/// </para>
/// <para>
/// <b>Time is passed in, never read from a clock.</b> That is what lets <c>TransferRateTests</c>
/// assert on exact figures rather than sleeping and hoping.
/// </para>
/// </remarks>
public sealed class TransferRate
{
    /// <summary>Weight given to the newest sample. Low enough to ride out a stutter, high enough
    /// that stopping shows up within a couple of seconds.</summary>
    private const double Smoothing = 0.25;

    /// <summary>Below this, an interval is too short to divide by: the timer's own resolution
    /// would dominate the answer.</summary>
    private static readonly TimeSpan MinimumSample = TimeSpan.FromMilliseconds(250);

    /// <summary>An estimate beyond this is not information. Better to say nothing.</summary>
    private static readonly TimeSpan LongestUsefulEstimate = TimeSpan.FromHours(99);

    private bool _seeded;
    private TimeSpan _lastAt;
    private long _lastBytes;
    private double? _bytesPerSecond;

    /// <summary>The smoothed throughput, or null until enough has been seen to say.</summary>
    public double? BytesPerSecond => _bytesPerSecond;

    /// <summary>
    /// Feeds in the cumulative byte count at a point in time. Samples closer together than
    /// <see cref="MinimumSample"/> are held rather than dropped, so a fast reporter still
    /// contributes its bytes to the next interval instead of being thrown away.
    /// </summary>
    public void Observe(long bytesDone, TimeSpan elapsed)
    {
        if (!_seeded)
        {
            _seeded = true;
            _lastAt = elapsed;
            _lastBytes = bytesDone;
            return;
        }

        var interval = elapsed - _lastAt;
        if (interval < MinimumSample) return;

        // A count that went backwards means a new transfer reusing this instance; re-seed rather
        // than reporting a negative rate.
        var moved = bytesDone - _lastBytes;
        if (moved < 0)
        {
            Reset();
            Observe(bytesDone, elapsed);
            return;
        }

        var sample = moved / interval.TotalSeconds;
        _bytesPerSecond = _bytesPerSecond is { } previous
            ? (previous * (1 - Smoothing)) + (sample * Smoothing)
            : sample;

        _lastAt = elapsed;
        _lastBytes = bytesDone;
    }

    /// <summary>
    /// How long the rest should take, or null when there is no honest answer: no throughput yet, a
    /// stalled transfer, or — the one worth being strict about — a byte total the size index could
    /// not complete, where the remainder is unknown rather than small.
    /// </summary>
    public TimeSpan? Remaining(long bytesDone, TransferEstimate estimate)
    {
        if (!estimate.IsUsable) return null;
        if (_bytesPerSecond is not { } rate || rate <= 0) return null;

        var left = estimate.Bytes - bytesDone;
        if (left <= 0) return TimeSpan.Zero;

        var seconds = left / rate;
        return seconds > LongestUsefulEstimate.TotalSeconds
            ? null
            : TimeSpan.FromSeconds(seconds);
    }

    public void Reset()
    {
        _seeded = false;
        _lastAt = TimeSpan.Zero;
        _lastBytes = 0;
        _bytesPerSecond = null;
    }
}
