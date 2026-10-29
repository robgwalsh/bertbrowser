namespace BertBrowser.Core.Services.Preview.Animation;

/// <summary>
/// How much of an animation the preview is willing to hold decoded. Every frame is kept as a
/// full canvas so a seek is a lookup, which makes the cost frames × width × height × 4 — past
/// these the still first frame is shown instead, as it always was.
/// </summary>
public static class AnimationLimits
{
    public const int MaxFrames = 2_000;

    public const long MaxDecodedBytes = 256L << 20; // 256 MB

    /// <summary>Whether an animation of this shape may be decoded whole. Asked of the header,
    /// before a single frame's pixels are read.</summary>
    public static bool Allows(int frameCount, int width, int height) =>
        frameCount is > 1 and <= MaxFrames
        && width > 0 && height > 0
        && (long)frameCount * width * height * 4 <= MaxDecodedBytes;
}
