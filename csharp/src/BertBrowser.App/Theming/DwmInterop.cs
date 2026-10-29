using System.Runtime.InteropServices;
using System.Windows.Media;

namespace BertBrowser.App.Theming;

/// <summary>
/// The bits of the window frame WPF does not own. Even with a custom title bar, DWM still draws a
/// hairline border, the resize edges, and the system menu, and paints a sliver of caption during a
/// resize — all of which stay light unless told otherwise.
/// </summary>
/// <remarks>
/// Every one of these attributes is unsupported on some Windows build and returns an error there.
/// That is expected and ignored: they are polish, not function.
/// </remarks>
internal static class DwmInterop
{
    private const int UseImmersiveDarkMode = 20;
    private const int UseImmersiveDarkModeBefore20H1 = 19;
    private const int WindowCornerPreference = 33;
    private const int BorderColor = 34;
    private const int CaptionColor = 35;
    private const int TextColor = 36;

    private const int RoundedCorners = 2;

    /// <summary>Applies the current theme to the parts of the frame DWM draws.</summary>
    public static void Apply(IntPtr hwnd, bool isDark, Color border, Color caption, Color text)
    {
        if (hwnd == IntPtr.Zero) return;

        var dark = isDark ? 1 : 0;
        // 20 is the documented attribute; 19 is what the first builds that had it used.
        if (!TrySet(hwnd, UseImmersiveDarkMode, dark))
            TrySet(hwnd, UseImmersiveDarkModeBefore20H1, dark);

        TrySet(hwnd, WindowCornerPreference, RoundedCorners);
        TrySet(hwnd, BorderColor, ToColorRef(border));
        TrySet(hwnd, CaptionColor, ToColorRef(caption));
        TrySet(hwnd, TextColor, ToColorRef(text));
    }

    /// <summary>COLORREF is 0x00BBGGRR — the reverse of the ARGB order everything else here uses.</summary>
    private static int ToColorRef(Color color) => color.R | (color.G << 8) | (color.B << 16);

    private static bool TrySet(IntPtr hwnd, int attribute, int value)
    {
        try
        {
            return DwmSetWindowAttribute(hwnd, attribute, ref value, sizeof(int)) == 0;
        }
        catch (DllNotFoundException)
        {
            return false;
        }
        catch (EntryPointNotFoundException)
        {
            return false;
        }
    }

    [DllImport("dwmapi.dll", ExactSpelling = true)]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);
}
