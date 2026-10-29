using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace BertBrowser.App.Interop;

/// <summary>
/// Small shell icons resolved by file extension (no disk access thanks to
/// SHGFI_USEFILEATTRIBUTES), cached as frozen ImageSources so they can be
/// created off the UI thread and shared across items.
/// </summary>
public static class ShellIcons
{
    // Directory and by-extension icons: a finite key set ("<dir>", "<none>", ".txt", …)
    // resolved from the registry without touching disk, so this can grow no larger than the
    // number of distinct extensions on the machine.
    private static readonly ConcurrentDictionary<string, ImageSource?> Cache = new(StringComparer.OrdinalIgnoreCase);

    // Per-file icons (.exe/.ico/.lnk) are extracted from each file and keyed by full path, so an
    // unbounded cache would leak steadily over a long session (every executable/shortcut/icon
    // file ever browsed stays pinned). Cap it with LRU eviction.
    private const int PerFileCacheCap = 512;
    private static readonly LruIconCache PerFileCache = new(PerFileCacheCap);

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

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SHSTOCKICONINFO
    {
        public uint cbSize;
        public IntPtr hIcon;
        public int iSysImageIndex;
        public int iIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szPath;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetStockIconInfo(uint siid, uint uFlags, ref SHSTOCKICONINFO psii);

    private const uint SHGSI_ICON = 0x100;
    private const uint SIID_DESKTOPPC = 15; // a generic computer/device icon

    private static ImageSource? _computerIcon;

    /// <summary>A generic "device" icon for portable devices (phones/cameras) that have
    /// no filesystem path to resolve a real icon from. Cached and frozen.</summary>
    public static ImageSource? GetComputerIcon()
    {
        if (_computerIcon is not null) return _computerIcon;

        var info = new SHSTOCKICONINFO { cbSize = (uint)Marshal.SizeOf<SHSTOCKICONINFO>() };
        if (SHGetStockIconInfo(SIID_DESKTOPPC, SHGSI_ICON | SHGFI_SMALLICON, ref info) != 0 ||
            info.hIcon == IntPtr.Zero)
            return null;

        try
        {
            var source = Imaging.CreateBitmapSourceFromHIcon(
                info.hIcon, Int32Rect.Empty, BitmapSizeOptions.FromEmptyOptions());
            source.Freeze();
            return _computerIcon = source;
        }
        finally
        {
            DestroyIcon(info.hIcon);
        }
    }

    /// <summary>True for the file types whose icon is extracted from the file itself
    /// (executables, shortcuts, icon files) — a disk hit that can stall (e.g. a .lnk pointing
    /// at a dead network share), so callers must resolve these off the UI thread.</summary>
    public static bool IsPerFileIcon(string path, bool isDirectory)
    {
        if (isDirectory) return false;
        var ext = Path.GetExtension(path);
        return ext.Equals(".exe", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".ico", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".lnk", StringComparison.OrdinalIgnoreCase);
    }

    public static ImageSource? GetIcon(string path, bool isDirectory)
    {
        // Executables/shortcuts/icon files carry their own icon: resolve from the file (disk)
        // and cache by full path in the bounded LRU.
        if (IsPerFileIcon(path, isDirectory))
            return PerFileCache.GetOrAdd(path, p => Load(p, isDirectory: false, fromDisk: true));

        // Directories share one icon; everything else is cached per extension, resolved from
        // file attributes only (no disk access).
        var ext = Path.GetExtension(path);
        var cacheKey = isDirectory ? "<dir>" : ext.Length == 0 ? "<none>" : ext;
        return Cache.GetOrAdd(cacheKey, key => Load(key, isDirectory, fromDisk: false));
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

    /// <summary>A small, thread-safe LRU of frozen icons keyed by path. The (potentially slow)
    /// factory runs outside the lock so one stalled shell call can't block other icon threads.</summary>
    private sealed class LruIconCache(int capacity)
    {
        private readonly object _gate = new();
        private readonly Dictionary<string, LinkedListNode<(string Key, ImageSource? Icon)>> _map =
            new(StringComparer.OrdinalIgnoreCase);
        private readonly LinkedList<(string Key, ImageSource? Icon)> _order = new();

        public ImageSource? GetOrAdd(string key, Func<string, ImageSource?> factory)
        {
            lock (_gate)
            {
                if (_map.TryGetValue(key, out var hit))
                {
                    _order.Remove(hit);
                    _order.AddFirst(hit);
                    return hit.Value.Icon;
                }
            }

            var icon = factory(key);

            lock (_gate)
            {
                // Another thread may have resolved the same key while we were outside the lock.
                if (_map.TryGetValue(key, out var existing))
                {
                    _order.Remove(existing);
                    _order.AddFirst(existing);
                    return existing.Value.Icon;
                }

                var node = new LinkedListNode<(string, ImageSource?)>((key, icon));
                _order.AddFirst(node);
                _map[key] = node;
                if (_map.Count > capacity)
                {
                    var lru = _order.Last!;
                    _order.RemoveLast();
                    _map.Remove(lru.Value.Key);
                }
                return icon;
            }
        }
    }
}
