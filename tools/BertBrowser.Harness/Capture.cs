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
        var (width, height) = RenderSize(element);

        if (width <= 0 || height <= 0)
            throw new InvalidOperationException(
                $"{Describe(element)} has no size to capture ({width}x{height}); it is probably collapsed.");

        var target = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        target.Render(AtOrigin(element, width, height));

        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(target));

        if (Path.GetDirectoryName(path) is { Length: > 0 } directory)
            Directory.CreateDirectory(directory);

        using var stream = File.Create(path);
        encoder.Save(stream);

        return (width, height);
    }

    /// <summary>
    /// Re-hosts an element at the origin so its picture is not shifted by where it sits.
    /// </summary>
    /// <remarks>
    /// <see cref="RenderTargetBitmap.Render"/> applies the visual's offset within its parent, so
    /// rendering a child straight into a bitmap of that child's size draws it at its window
    /// coordinates — the file list, three hundred pixels in from the left, lands mostly outside the
    /// bitmap and comes back as a band of empty background. Painting it through a
    /// <see cref="VisualBrush"/> normalises that away; the brush samples the live visual, so this
    /// is still a software re-render of the real tree rather than a copy of anything.
    /// </remarks>
    private static Visual AtOrigin(FrameworkElement element, int width, int height)
    {
        var visual = new DrawingVisual();

        using (var context = visual.RenderOpen())
        {
            context.DrawRectangle(
                new VisualBrush(element)
                {
                    Stretch = Stretch.None,
                    AlignmentX = AlignmentX.Left,
                    AlignmentY = AlignmentY.Top,
                },
                null,
                new Rect(0, 0, width, height));
        }

        return visual;
    }

    /// <summary>
    /// How big the picture of an element should be.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The element's own <see cref="FrameworkElement.RenderSize"/> — what layout actually gave it,
    /// not what it asked for (<c>DesiredSize</c>) and not what it would like to be (<c>Width</c>).
    /// </para>
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
