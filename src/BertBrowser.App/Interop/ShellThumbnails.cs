using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertBrowser.App.Interop;

/// <summary>
/// Real Explorer-style thumbnails (image/video previews, document thumbnails) with an
/// automatic file-type-icon fallback, via <c>IShellItemImageFactory</c>. Images come back
/// frozen so they can be produced on a worker thread and handed to the UI. Unlike
/// <see cref="ShellIcons"/> this touches disk, so callers should fetch off the UI thread.
/// </summary>
public static class ShellThumbnails
{
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    private static extern void SHCreateItemFromParsingName(
        string pszPath, IntPtr pbc, in Guid riid,
        [MarshalAs(UnmanagedType.Interface)] out IShellItemImageFactory ppv);

    [DllImport("gdi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeleteObject(IntPtr hObject);

    private static readonly Guid IID_IShellItemImageFactory =
        new("bcc18b79-ba16-442f-80c4-8a59c30c463b");

    [StructLayout(LayoutKind.Sequential)]
    private struct SIZE
    {
        public int cx;
        public int cy;
        public SIZE(int cx, int cy) { this.cx = cx; this.cy = cy; }
    }

    [Flags]
    private enum SIIGBF
    {
        ResizeToFit = 0x00,
        BiggerSizeOk = 0x01,
        MemoryOnly = 0x02,
        IconOnly = 0x04,
        ThumbnailOnly = 0x08,
        InCacheOnly = 0x10,
    }

    [ComImport, Guid("bcc18b79-ba16-442f-80c4-8a59c30c463b"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellItemImageFactory
    {
        void GetImage([MarshalAs(UnmanagedType.Struct)] SIZE size, SIIGBF flags, out IntPtr phbm);
    }

    /// <summary>Returns a thumbnail (or the file-type icon when no preview exists) sized to
    /// fit <paramref name="size"/>×<paramref name="size"/> pixels, or null on failure.</summary>
    public static ImageSource? GetThumbnail(string path, int size)
    {
        IShellItemImageFactory? factory = null;
        var hbitmap = IntPtr.Zero;
        try
        {
            SHCreateItemFromParsingName(path, IntPtr.Zero, IID_IShellItemImageFactory, out factory);
            // ResizeToFit gives a preview when the handler has one and an icon otherwise.
            factory.GetImage(new SIZE(size, size), SIIGBF.ResizeToFit, out hbitmap);
            if (hbitmap == IntPtr.Zero) return null;

            var source = Imaging.CreateBitmapSourceFromHBitmap(
                hbitmap, IntPtr.Zero, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            source.Freeze();
            return source;
        }
        catch (Exception ex) when (ex is COMException or FileNotFoundException or ArgumentException)
        {
            return null; // no thumbnail handler, path gone, etc. — caller falls back to the icon
        }
        finally
        {
            if (hbitmap != IntPtr.Zero) DeleteObject(hbitmap);
            if (factory is not null) Marshal.ReleaseComObject(factory);
        }
    }
}
