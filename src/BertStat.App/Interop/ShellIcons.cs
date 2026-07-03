using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertStat.App.Interop;

/// <summary>
/// Small shell icons resolved by file extension (no disk access thanks to
/// SHGFI_USEFILEATTRIBUTES), cached as frozen ImageSources so they can be
/// created off the UI thread and shared across items.
/// </summary>
public static class ShellIcons
{
    private static readonly ConcurrentDictionary<string, ImageSource?> Cache = new(StringComparer.OrdinalIgnoreCase);

    private const uint SHGFI_ICON = 0x100;
    private const uint SHGFI_SMALLICON = 0x1;
    private const uint SHGFI_USEFILEATTRIBUTES = 0x10;
    private const uint FILE_ATTRIBUTE_NORMAL = 0x80;
    private const uint FILE_ATTRIBUTE_DIRECTORY = 0x10;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SHFILEINFO
    {
        public IntPtr hIcon;
        public int iIcon;
        public uint dwAttributes;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szDisplayName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 80)] public string szTypeName;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr SHGetFileInfo(
        string pszPath, uint dwFileAttributes, ref SHFILEINFO psfi, uint cbFileInfo, uint uFlags);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyIcon(IntPtr hIcon);

    public static ImageSource? GetIcon(string path, bool isDirectory)
    {
        // Directories share one icon; files are cached per extension except types
        // whose icon is per-file (executables, shortcuts, icon files).
        var ext = Path.GetExtension(path);
        var perFile = ext.Equals(".exe", StringComparison.OrdinalIgnoreCase)
                   || ext.Equals(".ico", StringComparison.OrdinalIgnoreCase)
                   || ext.Equals(".lnk", StringComparison.OrdinalIgnoreCase);

        var cacheKey = isDirectory ? "<dir>" : perFile ? path : ext.Length == 0 ? "<none>" : ext;
        return Cache.GetOrAdd(cacheKey, _ => Load(perFile ? path : cacheKey, isDirectory, perFile));
    }

    private static ImageSource? Load(string pathOrExt, bool isDirectory, bool fromDisk)
    {
        var info = new SHFILEINFO();
        var flags = SHGFI_ICON | SHGFI_SMALLICON | (fromDisk ? 0u : SHGFI_USEFILEATTRIBUTES);
        var attrs = isDirectory ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL;
        var query = isDirectory ? "folder" : pathOrExt is "<none>" or "<dir>" ? "file" : pathOrExt;

        var result = SHGetFileInfo(query, attrs, ref info, (uint)Marshal.SizeOf<SHFILEINFO>(), flags);
        if (result == IntPtr.Zero || info.hIcon == IntPtr.Zero)
            return null;

        try
        {
            var source = Imaging.CreateBitmapSourceFromHIcon(
                info.hIcon, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            source.Freeze();
            return source;
        }
        finally
        {
            DestroyIcon(info.hIcon);
        }
    }
}
