using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertBrowser.Harness;

/// <summary>
/// Turns a piece of the live visual tree into a PNG.
/// </summary>
/// <remarks>
/// <para>
/// <see cref="RenderTargetBitmap"/> walks the visual tree and rasterises it in software. It is not
/// a screen grab, which is the entire reason this works: the window it is capturing has never been
/// presented, sits outside every monitor, and could be covered by anything at all — a fullscreen
/// game included — and the picture is identical either way.
/// </para>
/// <para>
/// Captured at 96 DPI against the element's own device-independent size, so the file is the same
/// on a scaled monitor as on an unscaled one. <c>Render</c> applies the visual's own transforms but
/// not the window's device transform, so no scale correction belongs here.
/// </para>
/// </remarks>
internal static class Capture
{
    public static (int Width, int Height) Save(FrameworkElement element, string path)
    {
        var root = Root(element);
        var (width, height) = RenderSize(root);

        if (width <= 0 || height <= 0)
            throw new InvalidOperationException(
                $"{Describe(root)} has no size to capture ({width}x{height}).");

        var target = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        target.Render(root);

        BitmapSource picture = ReferenceEquals(element, root)
            ? target
            : CropTo(target, element, root);

        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(picture));

        if (Path.GetDirectoryName(path) is { Length: > 0 } directory)
            Directory.CreateDirectory(directory);

        using var stream = File.Create(path);
        encoder.Save(stream);

        return (picture.PixelWidth, picture.PixelHeight);
    }

    /// <summary>
    /// Cuts one element out of a picture of the whole window.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The obvious way to photograph a child on its own is to paint it through a
    /// <see cref="VisualBrush"/> into a bitmap of its size — <see cref="RenderTargetBitmap.Render"/>
    /// applies the visual's offset within its parent, so rendering a child straight into a bitmap
    /// of that child's size draws it at its window coordinates and mostly misses.
    /// </para>
    /// <para>
    /// That way is wrong here, and wrong in a way that takes a while to catch: WPF caches a
    /// <see cref="VisualBrush"/>'s realisation of the visual it points at, and a
    /// <see cref="SolidColorBrush"/> inside that visual changing colour does not invalidate the
    /// cache. So one capture taken before a <c>theme</c> command made every capture after it come
    /// back in the old theme's colours — while the brushes, the resources and the elements' own
    /// properties all said the new theme had applied. Rendering the root and cropping has no cache
    /// to go stale.
    /// </para>
    /// </remarks>
    private static BitmapSource CropTo(BitmapSource picture, FrameworkElement element, FrameworkElement root)
    {
        Rect bounds;
        try
        {
            bounds = element.TransformToAncestor(root).TransformBounds(new Rect(element.RenderSize));
        }
        catch (InvalidOperationException e)
        {
            throw new InvalidOperationException(
                $"{Describe(element)} is not connected to {Describe(root)}.", e);
        }

        // Clamped, because an element can extend past the window it is in — a list wider than its
        // viewport, a row scrolled half out — and CroppedBitmap throws rather than clipping.
        var x = (int)Math.Floor(Math.Clamp(bounds.X, 0, picture.PixelWidth));
        var y = (int)Math.Floor(Math.Clamp(bounds.Y, 0, picture.PixelHeight));
        var width = (int)Math.Ceiling(Math.Clamp(bounds.Right, 0, picture.PixelWidth)) - x;
        var height = (int)Math.Ceiling(Math.Clamp(bounds.Bottom, 0, picture.PixelHeight)) - y;

        if (width <= 0 || height <= 0)
            throw new InvalidOperationException(
                $"{Describe(element)} has no visible size to capture ({bounds}); it is probably " +
                "collapsed or scrolled out of the window.");

        return new CroppedBitmap(picture, new Int32Rect(x, y, width, height));
    }

    /// <summary>The window an element belongs to, or the element itself when it is one.</summary>
    private static FrameworkElement Root(FrameworkElement element)
    {
        var root = element;
        while (root is not Window && VisualTreeHelper.GetParent(root) is FrameworkElement parent)
            root = parent;

        return root;
    }

    /// <summary>
    /// How big the picture of a window should be.
    /// </summary>
    /// <remarks>
    /// The element's own <see cref="FrameworkElement.RenderSize"/> — what layout actually gave it,
    /// not what it asked for (<c>DesiredSize</c>) and not what it would like to be (<c>Width</c>).
    /// <para>
    /// It is tempting to measure a <see cref="Window"/> by its <c>Content</c> instead, since a
    /// window's <c>ActualHeight</c> normally includes a caption its visual tree does not contain.
    /// That is wrong here, and quietly: every window in this app draws its own title bar through
    /// <c>WindowChrome</c>, so the window's visual really does cover the whole frame — while
    /// <c>Content</c> sits below the caption and inside the root panel's margin. Measuring the
    /// content and painting the window produced a picture 34 px short at the bottom, and 32 px
    /// short again for any dialog whose root panel had a margin.
    /// </para>
    /// </remarks>
    private static (int Width, int Height) RenderSize(FrameworkElement element)
    {
        var size = element.RenderSize;

        return ((int)Math.Ceiling(size.Width), (int)Math.Ceiling(size.Height));
    }

    /// <summary>
    /// True when a capture contains more than one colour.
    /// </summary>
    /// <remarks>
    /// The failure this guards against is silent: an offscreen window that rendered nothing
    /// produces a perfectly valid PNG of a single flat colour, and every assertion downstream would
    /// pass while the pictures showed nothing at all.
    /// </remarks>
    public static bool HasContent(string path)
    {
        using var stream = File.OpenRead(path);
        var decoded = new PngBitmapDecoder(
            stream, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad).Frames[0];

        var converted = new FormatConvertedBitmap(decoded, PixelFormats.Bgra32, null, 0);
        var stride = converted.PixelWidth * 4;
        var pixels = new byte[stride * converted.PixelHeight];
        converted.CopyPixels(pixels, stride, 0);

        var first = BitConverter.ToUInt32(pixels, 0);
        for (var i = 4; i < pixels.Length; i += 4)
            if (BitConverter.ToUInt32(pixels, i) != first)
                return true;

        return false;
    }

    private static string Describe(FrameworkElement element) =>
        string.IsNullOrEmpty(element.Name) ? element.GetType().Name : element.Name;
}
