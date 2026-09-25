namespace BertBrowser.Core.Services.Preview;

/// <summary>
/// The arithmetic behind the preview pane's zoomable image, one axis at a time.
/// </summary>
/// <remarks>
/// The image is laid out fitted to the pane, then drawn through <c>p * scale + offset</c> in its
/// own coordinates — so a scale of 1 is "fit", and everything here is relative to that. The view
/// owns the transform; this owns the rules, so they can be tested without a window.
/// </remarks>
public static class ImageZoom
{
    /// <summary>How far past "fit" the wheel will go. Enough to count the pixels of a photo shown
    /// in a narrow pane.</summary>
    public const double MaxScale = 64;

    /// <summary>One wheel notch multiplies or divides the scale by this.</summary>
    public const double StepFactor = 1.2;

    /// <summary>
    /// The scale after <paramref name="notches"/> wheel notches (positive zooms in). Never below
    /// <paramref name="minScale"/> — normally 1, fit, since zooming out past the pane only shrinks
    /// the picture into a margin — and never above <see cref="MaxScale"/>.
    /// </summary>
    public static double Step(double scale, double notches, double minScale = 1) =>
        Clamp(scale * Math.Pow(StepFactor, notches), minScale);

    public static double Clamp(double scale, double minScale = 1) =>
        Math.Clamp(scale, Math.Min(minScale, MaxScale), MaxScale);

    /// <summary>
    /// The offset that keeps the image point under the cursor where it is when the scale changes.
    /// <paramref name="anchor"/> is in the image's own, untransformed coordinates.
    /// </summary>
    public static double ZoomOffset(double offset, double anchor, double oldScale, double newScale) =>
        offset + anchor * (oldScale - newScale);

    /// <summary>
    /// Keeps the picture from being dragged out of view. Along an axis where the scaled image is
    /// smaller than the pane it is centred; where it is larger, no edge may come inside the pane's.
    /// </summary>
    /// <param name="offset">The translation being proposed.</param>
    /// <param name="origin">Where layout put the unscaled image's leading edge within the pane.</param>
    /// <param name="length">The unscaled (fitted) image's length along this axis.</param>
    /// <param name="scale">The current scale.</param>
    /// <param name="viewport">The pane's length along this axis.</param>
    public static double ClampOffset(double offset, double origin, double length, double scale, double viewport)
    {
        var scaled = length * scale;
        if (scaled <= viewport) return (viewport - scaled) / 2 - origin;
        return Math.Clamp(offset, viewport - scaled - origin, -origin);
    }
}
