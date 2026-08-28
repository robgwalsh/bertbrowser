using System.Collections.Generic;
using System.Linq;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertBrowser.App.Views;

/// <summary>
/// Picks a frame out of <c>Assets/app.ico</c> at the size it will actually be drawn.
/// </summary>
/// <remarks>
/// <para>
/// The icon is three hand-drawn tiers, not one drawing scaled (see
/// <c>tools/icon/build-app-icon.ps1</c>), so which frame reaches the screen decides whether the
/// small sizes look drawn or smeared. WPF will not choose for us: <c>Icon="/Assets/app.ico"</c> in
/// XAML resolves through <c>BitmapFrame.Create</c>, which hands back one arbitrary frame — the
/// 64x64 one, as it happens — and then scales that to whatever the surface needs. A 64px frame
/// squeezed into a 16px slot is exactly the mush the tiers exist to avoid.
/// </para>
/// <para>
/// This is also why <see cref="System.Windows.Window.Icon"/> is deliberately left unset everywhere.
/// A null <c>Icon</c> is not a missing icon: WPF then lets Windows use the executable's own icon
/// resource for the taskbar and Alt+Tab, and the shell picks per size from all ten frames — better
/// than anything we could assign. Setting <c>Icon</c> would replace that with one scaled frame.
/// </para>
/// </remarks>
internal static class AppIcon
{
    // Assembly-qualified, not the bare "/Assets/app.ico" form. A relative pack URI resolves against
    // the *entry* assembly, which is BertBrowser.Harness when the harness hosts these windows — so
    // the short form throws there and the window fails to open at all, while working perfectly in
    // the app itself.
    private const string ResourceUri = "pack://application:,,,/BertBrowser;component/Assets/app.ico";

    private static IReadOnlyList<BitmapFrame>? _frames;

    /// <summary>
    /// The frame to draw in a slot <paramref name="dipSize"/> device-independent pixels across on a
    /// display scaled by <paramref name="dpiScale"/>.
    /// </summary>
    /// <remarks>
    /// Smallest frame at least as large as the slot, so the result is downscaled or exact and never
    /// blown up. The file carries 16, 20, 24, 32, 40 and 48 precisely so that the common scale
    /// factors — 100%, 125%, 150%, 200%, 250%, 300% — each land on a frame drawn for that size.
    /// </remarks>
    public static ImageSource? ForSlot(double dipSize, double dpiScale)
    {
        var wanted = (int)System.Math.Ceiling(dipSize * dpiScale);
        var frames = Frames();
        if (frames.Count == 0) return null;

        return frames.FirstOrDefault(f => f.PixelWidth >= wanted) ?? frames[^1];
    }

    private static IReadOnlyList<BitmapFrame> Frames()
    {
        if (_frames is not null) return _frames;

        try
        {
            var uri = new System.Uri(ResourceUri);
            var decoder = BitmapDecoder.Create(uri, BitmapCreateOptions.None, BitmapCacheOption.OnLoad);

            // Ordered by size because the decoder does not report them in the order the file stores
            // them, and ForSlot's "smallest that fits" depends on the order.
            _frames = decoder.Frames.OrderBy(f => f.PixelWidth).ToArray();
        }
        catch (System.Exception)
        {
            // Swallowed rather than propagated: this is reached from a dependency property setter
            // during XAML parse, and an exception there does not surface as "the icon is missing" —
            // it surfaces as the window failing to open, with a message naming the property. A
            // title bar with no icon is the right way to lose this.
            _frames = System.Array.Empty<BitmapFrame>();
        }

        return _frames;
    }
}
