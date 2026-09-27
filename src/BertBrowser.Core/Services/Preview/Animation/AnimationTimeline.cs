namespace BertBrowser.Core.Services.Preview.Animation;

/// <summary>
/// When each frame of an animation is on screen, and which frame a position lands on. Always
/// loops: a position past the end wraps, so the clock driving playback never has to reset.
/// </summary>
/// <remarks>
/// The GIF's own loop count is deliberately ignored — a preview that stops after three passes
/// reads as a frozen picture, and the seek bar is how someone gets a particular frame to hold.
/// </remarks>
public sealed class AnimationTimeline
{
    /// <summary>Delays at or below this are what browsers replace, and so what GIF authors have
    /// long relied on: a declared 0 or 10 ms plays at 100 ms everywhere else.</summary>
    public const int MinHonouredDelayMs = 10;

    /// <summary>What a delay too short to honour is played at.</summary>
    public const int DefaultDelayMs = 100;

    private readonly long[] _starts;

    public AnimationTimeline(IReadOnlyList<int> delaysMs)
    {
        if (delaysMs.Count == 0) throw new ArgumentException("An animation has at least one frame.", nameof(delaysMs));

        _starts = new long[delaysMs.Count];
        var delays = new int[delaysMs.Count];
        long at = 0;
        for (var i = 0; i < delaysMs.Count; i++)
        {
            _starts[i] = at;
            delays[i] = delaysMs[i] <= MinHonouredDelayMs ? DefaultDelayMs : delaysMs[i];
            at += delays[i];
        }
        Delays = delays;
        Duration = at;
    }

    /// <summary>Each frame's delay after the too-short ones have been replaced.</summary>
    public IReadOnlyList<int> Delays { get; }

    public int FrameCount => _starts.Length;

    /// <summary>One pass, in milliseconds. Never zero, since every delay is at least
    /// <see cref="DefaultDelayMs"/> or above <see cref="MinHonouredDelayMs"/>.</summary>
    public long Duration { get; }

    /// <summary>A position folded into one pass. Negative positions wrap from the end.</summary>
    public long Wrap(long positionMs)
    {
        var p = positionMs % Duration;
        return p < 0 ? p + Duration : p;
    }

    public long StartOf(int frame) => _starts[Math.Clamp(frame, 0, _starts.Length - 1)];

    /// <summary>The frame on screen at <paramref name="positionMs"/>, after wrapping.</summary>
    public int FrameAt(long positionMs)
    {
        var p = Wrap(positionMs);
        var index = Array.BinarySearch(_starts, p);
        return index >= 0 ? index : ~index - 1;
    }

    /// <summary>How long from <paramref name="positionMs"/> until the frame changes. Always at
    /// least 1, so a timer set from it can never spin.</summary>
    public long UntilNextFrame(long positionMs)
    {
        var p = Wrap(positionMs);
        var frame = FrameAt(p);
        var end = frame + 1 < _starts.Length ? _starts[frame + 1] : Duration;
        return Math.Max(1, end - p);
    }
}
