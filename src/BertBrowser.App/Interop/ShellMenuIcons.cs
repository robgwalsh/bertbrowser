using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertBrowser.App.Interop;

/// <summary>
/// The pictures a shell extension puts beside its items, as WPF images.
/// </summary>
/// <remarks>
/// A handler's menu bitmap is a 32-bit premultiplied-alpha <c>HBITMAP</c> (the shell has required
/// that since Vista), and <c>Imaging.CreateBitmapSourceFromHBitmap</c> throws that alpha channel
/// away — every icon would sit in a black square. So the pixels are read out with <c>GetDIBits</c>
/// and handed to WPF as <c>Pbgra32</c>, which is the same premultiplied layout. Icons named by a
/// static verb's <c>Icon</c> value come as <c>HICON</c>s, for which the built-in conversion is fine.
/// </remarks>
internal static class ShellMenuIcons
{
    /// <summary><c>hbmpItem</c> values at or below this are <c>HBMMENU_*</c> sentinels asking for a
    /// stock picture, not bitmaps.</summary>
    private const long LastMenuSentinel = 11;

    private const uint DibRgbColors = 0;
    private const int MaxSide = 256;

    /// <summary>A menu item's bitmap, or null for a sentinel, a bitmap that cannot be read, or
    /// one too large to be an icon.</summary>
    public static ImageSource? FromMenuBitmap(IntPtr hbitmap)
    {
        if (hbitmap.ToInt64() <= LastMenuSentinel) return null;

        var bitmap = default(BITMAP);
        if (GetObjectW(hbitmap, Marshal.SizeOf<BITMAP>(), ref bitmap) == 0) return null;

        var width = bitmap.bmWidth;
        var height = bitmap.bmHeight;
        if (width <= 0 || height <= 0 || width > MaxSide || height > MaxSide) return null;

        var info = new BITMAPINFO
        {
            biSize = 40,
            biWidth = width,
            biHeight = -height, // top-down, the way WPF reads rows
            biPlanes = 1,
            biBitCount = 32,
        };
        var pixels = new byte[width * height * 4];

        var hdc = GetDC(IntPtr.Zero);
        if (hdc == IntPtr.Zero) return null;
        try
        {
            if (GetDIBits(hdc, hbitmap, 0, (uint)height, pixels, ref info, DibRgbColors) == 0) return null;
        }
        finally
        {
            ReleaseDC(IntPtr.Zero, hdc);
        }

        // A bitmap without an alpha channel comes back with every alpha byte zero, which Pbgra32
        // would draw as nothing at all.
        if (bitmap.bmBitsPixel < 32 || AllAlphaZero(pixels))
        {
            for (var i = 3; i < pixels.Length; i += 4) pixels[i] = 255;
        }

        var source = BitmapSource.Create(width, height, 96, 96, PixelFormats.Pbgra32, null, pixels, width * 4);
        source.Freeze();
        return source;
    }

    /// <summary>The small icon a verb's <c>Icon</c> value names — <c>path</c>, <c>path,index</c>
    /// or <c>path,-resourceId</c>, environment variables and quotes included — or null.</summary>
    public static ImageSource? FromIconResource(string? iconValue)
    {
        if (string.IsNullOrWhiteSpace(iconValue)) return null;

        var text = Environment.ExpandEnvironmentVariables(iconValue.Trim());
        var index = 0;
        var comma = text.LastIndexOf(',');
        if (comma > 0 && int.TryParse(text[(comma + 1)..].Trim(), out var parsed))
        {
            index = parsed;
            text = text[..comma];
        }

        text = text.Trim().Trim('"');
        if (text.Length == 0) return null;

        var large = IntPtr.Zero;
        var small = IntPtr.Zero;
        try
        {
            // Sizes packed as MAKELONG(large, small); S_FALSE means the file had no icon.
            if (SHDefExtractIconW(text, index, 0, ref large, ref small, (16 << 16) | 32) != 0) return null;
            if (small == IntPtr.Zero) return null;

            var source = Imaging.CreateBitmapSourceFromHIcon(
                small, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            source.Freeze();
            return source;
        }
        catch (Exception ex) when (ex is COMException or ArgumentException or Win32Exception)
        {
            return null;
        }
        finally
        {
            if (large != IntPtr.Zero) DestroyIcon(large);
            if (small != IntPtr.Zero) DestroyIcon(small);
        }
    }

    private static bool AllAlphaZero(byte[] pixels)
    {
        for (var i = 3; i < pixels.Length; i += 4)
        {
            if (pixels[i] != 0) return false;
        }

        return true;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BITMAP
    {
        public int bmType;
        public int bmWidth;
        public int bmHeight;
        public int bmWidthBytes;
        public ushort bmPlanes;
        public ushort bmBitsPixel;
        public IntPtr bmBits;
    }

    /// <summary>A <c>BITMAPINFOHEADER</c> with the one colour slot the struct always carries.</summary>
    [StructLayout(LayoutKind.Sequential)]
    private struct BITMAPINFO
    {
        public uint biSize;
        public int biWidth;
        public int biHeight;
        public ushort biPlanes;
        public ushort biBitCount;
        public uint biCompression;
        public uint biSizeImage;
        public int biXPelsPerMeter;
        public int biYPelsPerMeter;
        public uint biClrUsed;
        public uint biClrImportant;
        public uint bmiColors;
    }

    [DllImport("gdi32.dll", ExactSpelling = true)]
    private static extern int GetObjectW(IntPtr h, int c, ref BITMAP pv);

    [DllImport("gdi32.dll", ExactSpelling = true)]
    private static extern int GetDIBits(
        IntPtr hdc, IntPtr hbm, uint start, uint cLines, [Out] byte[] lpvBits, ref BITMAPINFO lpbmi, uint usage);

    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern IntPtr GetDC(IntPtr hWnd);

    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern int ReleaseDC(IntPtr hWnd, IntPtr hDC);

    [DllImport("user32.dll", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyIcon(IntPtr hIcon);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int SHDefExtractIconW(
        string pszIconFile, int iIndex, uint uFlags, ref IntPtr phiconLarge, ref IntPtr phiconSmall, uint nIconSize);
}
