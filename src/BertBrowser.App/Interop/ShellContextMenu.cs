using System.Runtime.InteropServices;
using System.Windows.Media;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Interop;

/// <summary>
/// One COM context-menu handler — 7-Zip's, TortoiseSVN's — hosted the way Explorer hosts it:
/// created, initialised with the selection, asked to fill a menu, and later asked to run one of its
/// items.
/// </summary>
/// <remarks>
/// <para>
/// The handler fills a real Win32 <c>HMENU</c>, because that is the only thing the interface
/// offers; nothing ever shows that menu. It is walked and mirrored into <see cref="ShellMenuEntry"/>
/// rows for the app's own WPF menu, and an entry's <see cref="Command"/> remembers which handler and
/// which id to hand back to <c>InvokeCommand</c>. Each handler gets its own <c>HMENU</c> with ids
/// from <see cref="IdFirst"/>, so ids never collide across handlers and nothing needs to remember
/// whose range an id fell in.
/// </para>
/// <para>
/// Extensions that build a submenu lazily do it in response to <c>WM_INITMENUPOPUP</c>, which
/// Windows would send while the menu is up. Nothing is up, so the message is delivered by hand
/// through <c>IContextMenu2</c>/<c>3</c> before each submenu is walked.
/// </para>
/// <para>
/// All of it on the UI thread: shell extensions are apartment-threaded, as <see cref="ShellLink"/>
/// notes for the shell's own classes. A handler that faults natively takes the process down, here
/// as in Explorer; the per-extension checklist in Settings is the remedy for that, and the reason
/// the host never hides which extension an item came from.
/// </para>
/// </remarks>
internal sealed class ShellContextMenuHandler : IDisposable
{
    /// <summary>What an entry carries so the session can invoke it.</summary>
    public sealed record Command(ShellContextMenuHandler Handler, uint Id);

    private const uint IdFirst = 1;
    private const uint IdLast = 0x7FFF;
    private const uint CmfNormal = 0;

    private const uint MiimState = 0x1;
    private const uint MiimId = 0x2;
    private const uint MiimSubmenu = 0x4;
    private const uint MiimString = 0x40;
    private const uint MiimBitmap = 0x80;
    private const uint MiimFType = 0x100;

    private const uint MftBitmap = 0x4;
    private const uint MftOwnerDraw = 0x100;
    private const uint MftSeparator = 0x800;
    private const uint MfsDisabledOrGrayed = 0x3;

    private const uint WmInitMenuPopup = 0x117;
    private const uint GcsVerbW = 0x4;
    private const uint CmicMaskUnicode = 0x4000;
    private const int SwShowNormal = 1;

    private readonly object _instance;
    private readonly IContextMenu _menu;
    private readonly IContextMenu2? _menu2;
    private readonly IContextMenu3? _menu3;
    private IntPtr _hmenu;
    private bool _disposed;

    private ShellContextMenuHandler(object instance, IContextMenu menu)
    {
        _instance = instance;
        _menu = menu;
        _menu2 = instance as IContextMenu2;
        _menu3 = instance as IContextMenu3;
    }

    /// <summary>
    /// Creates and initialises the handler, or returns null when it cannot be created, does not
    /// implement the two interfaces, or refuses the selection — every one of which means "this
    /// extension contributes nothing this time", never "the menu failed".
    /// </summary>
    /// <param name="hkeyProgId">The registry key the handler was found under, which some handlers
    /// read their own settings from. Zero is allowed.</param>
    public static ShellContextMenuHandler? Create(Guid clsid, ShellSelection selection, IntPtr hkeyProgId)
    {
        object? instance = null;
        try
        {
            var type = Type.GetTypeFromCLSID(clsid, throwOnError: false);
            if (type is null) return null;

            instance = Activator.CreateInstance(type);
            if (instance is not IShellExtInit init || instance is not IContextMenu menu)
            {
                Release(instance);
                return null;
            }

            if (init.Initialize(selection.FolderPidl, selection.DataObject, hkeyProgId) < 0)
            {
                Release(instance);
                return null;
            }

            return new ShellContextMenuHandler(instance, menu);
        }
        catch (Exception ex) when (IsHandlerFailure(ex))
        {
            // Third-party code, failing in whichever way it likes: an unregistered class, a DLL
            // that will not load, a cast the object refuses. The item is not offered.
            Release(instance);
            return null;
        }
    }

    /// <summary>Asks the handler for its items and mirrors them. Empty when it added none.</summary>
    public IReadOnlyList<ShellMenuEntry> Query(Func<IntPtr, ImageSource?> imageOf)
    {
        if (_hmenu != IntPtr.Zero) DestroyMenu(_hmenu);
        _hmenu = CreatePopupMenu();
        if (_hmenu == IntPtr.Zero) return [];

        try
        {
            if (_menu.QueryContextMenu(_hmenu, 0, IdFirst, IdLast, CmfNormal) < 0) return [];
            return Walk(_hmenu, imageOf);
        }
        catch (Exception ex) when (IsHandlerFailure(ex))
        {
            return [];
        }
    }

    /// <summary>Runs one of the handler's items. Returns the HRESULT; the caller words it.</summary>
    public int Invoke(uint id, IntPtr ownerWindow, string folder)
    {
        var directoryAnsi = IntPtr.Zero;
        var directoryWide = IntPtr.Zero;
        try
        {
            directoryAnsi = Marshal.StringToHGlobalAnsi(folder);
            directoryWide = Marshal.StringToHGlobalUni(folder);

            var info = new CMINVOKECOMMANDINFOEX
            {
                cbSize = (uint)Marshal.SizeOf<CMINVOKECOMMANDINFOEX>(),
                fMask = CmicMaskUnicode,
                hwnd = ownerWindow,
                lpVerb = (IntPtr)(id - IdFirst),
                lpVerbW = (IntPtr)(id - IdFirst),
                lpDirectory = directoryAnsi,
                lpDirectoryW = directoryWide,
                nShow = SwShowNormal,
            };

            return _menu.InvokeCommand(ref info);
        }
        catch (Exception ex) when (IsHandlerFailure(ex))
        {
            return Marshal.GetHRForException(ex);
        }
        finally
        {
            if (directoryAnsi != IntPtr.Zero) Marshal.FreeHGlobal(directoryAnsi);
            if (directoryWide != IntPtr.Zero) Marshal.FreeHGlobal(directoryWide);
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_hmenu != IntPtr.Zero)
        {
            DestroyMenu(_hmenu);
            _hmenu = IntPtr.Zero;
        }

        Release(_instance);
    }

    private List<ShellMenuEntry> Walk(IntPtr hmenu, Func<IntPtr, ImageSource?> imageOf)
    {
        var entries = new List<ShellMenuEntry>();
        var count = GetMenuItemCount(hmenu);

        for (var position = 0; position < count; position++)
        {
            var info = new MENUITEMINFO
            {
                cbSize = (uint)Marshal.SizeOf<MENUITEMINFO>(),
                fMask = MiimState | MiimId | MiimSubmenu | MiimString | MiimBitmap | MiimFType,
            };
            if (!GetMenuItemInfoW(hmenu, (uint)position, true, ref info)) continue;

            if ((info.fType & MftSeparator) != 0)
            {
                entries.Add(ShellMenuEntry.Separator);
                continue;
            }

            var header = ShellMenuRules.Header(TextOf(hmenu, position, info));
            var enabled = (info.fState & MfsDisabledOrGrayed) == 0;
            var icon = imageOf(info.hbmpItem);

            if (info.hSubMenu != IntPtr.Zero)
            {
                // What Windows would send as the submenu opened, so a handler that fills it then
                // has filled it before it is read.
                HandleMenuMessage(WmInitMenuPopup, info.hSubMenu, (IntPtr)position);
                entries.Add(ShellMenuEntry.Submenu(header, Walk(info.hSubMenu, imageOf), icon));
                continue;
            }

            entries.Add(ShellMenuEntry.Item(header, new Command(this, info.wID), enabled, icon));
        }

        return entries;
    }

    /// <summary>The item's text: its string for an ordinary item, its verb for an owner-drawn one
    /// (whose string the handler keeps to itself), or empty.</summary>
    private string TextOf(IntPtr hmenu, int position, MENUITEMINFO info)
    {
        if ((info.fType & (MftBitmap | MftOwnerDraw)) == 0 && info.cch > 0)
        {
            var length = info.cch + 1;
            var buffer = Marshal.AllocHGlobal((int)length * 2);
            try
            {
                var text = new MENUITEMINFO
                {
                    cbSize = (uint)Marshal.SizeOf<MENUITEMINFO>(),
                    fMask = MiimString,
                    dwTypeData = buffer,
                    cch = length,
                };
                if (GetMenuItemInfoW(hmenu, (uint)position, true, ref text))
                    return Marshal.PtrToStringUni(buffer) ?? "";
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }

        if ((info.fType & MftOwnerDraw) != 0 && info.hSubMenu == IntPtr.Zero)
            return VerbOf(info.wID);

        return "";
    }

    private string VerbOf(uint id)
    {
        const int capacity = 256;
        var buffer = Marshal.AllocHGlobal(capacity * 2);
        try
        {
            return _menu.GetCommandString((UIntPtr)(id - IdFirst), GcsVerbW, IntPtr.Zero, buffer, capacity) == 0
                ? Marshal.PtrToStringUni(buffer) ?? ""
                : "";
        }
        catch (Exception ex) when (IsHandlerFailure(ex))
        {
            return "";
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private void HandleMenuMessage(uint message, IntPtr wParam, IntPtr lParam)
    {
        try
        {
            if (_menu3 is not null) _menu3.HandleMenuMsg2(message, wParam, lParam, out _);
            else _menu2?.HandleMenuMsg(message, wParam, lParam);
        }
        catch (Exception ex) when (IsHandlerFailure(ex))
        {
            // The submenu is read as it stands.
        }
    }

    private static void Release(object? instance)
    {
        if (instance is not null && Marshal.IsComObject(instance)) Marshal.FinalReleaseComObject(instance);
    }

    /// <summary>The ways a foreign in-process object has been seen to fail short of faulting.</summary>
    private static bool IsHandlerFailure(Exception ex) =>
        ex is COMException or InvalidCastException or InvalidOperationException
            or MissingMethodException or MemberAccessException or NotSupportedException
            or FileNotFoundException or BadImageFormatException or TypeLoadException
            or AccessViolationException or ArgumentException or NullReferenceException;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct MENUITEMINFO
    {
        public uint cbSize;
        public uint fMask;
        public uint fType;
        public uint fState;
        public uint wID;
        public IntPtr hSubMenu;
        public IntPtr hbmpChecked;
        public IntPtr hbmpUnchecked;
        public IntPtr dwItemData;
        public IntPtr dwTypeData;
        public uint cch;
        public IntPtr hbmpItem;
    }

    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DestroyMenu(IntPtr hMenu);

    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern int GetMenuItemCount(IntPtr hMenu);

    [DllImport("user32.dll", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetMenuItemInfoW(
        IntPtr hmenu, uint item, [MarshalAs(UnmanagedType.Bool)] bool fByPosition, ref MENUITEMINFO lpmii);
}

/// <summary>The Unicode invoke record; <c>cbSize</c> tells the handler which shape it got.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct CMINVOKECOMMANDINFOEX
{
    public uint cbSize;
    public uint fMask;
    public IntPtr hwnd;
    public IntPtr lpVerb;
    public IntPtr lpParameters;
    public IntPtr lpDirectory;
    public int nShow;
    public uint dwHotKey;
    public IntPtr hIcon;
    public IntPtr lpTitle;
    public IntPtr lpVerbW;
    public IntPtr lpParametersW;
    public IntPtr lpDirectoryW;
    public IntPtr lpTitleW;
    public int ptInvokeX;
    public int ptInvokeY;
}

[ComImport, Guid("000214E8-0000-0000-C000-000000000046"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IShellExtInit
{
    [PreserveSig] int Initialize(IntPtr pidlFolder, IntPtr pdtobj, IntPtr hkeyProgID);
}

/// <summary>Declared three times over rather than by inheritance: a <c>ComImport</c> interface's
/// vtable is its own method list, so each version repeats the slots before its additions.</summary>
[ComImport, Guid("000214E4-0000-0000-C000-000000000046"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IContextMenu
{
    [PreserveSig] int QueryContextMenu(IntPtr hmenu, uint indexMenu, uint idCmdFirst, uint idCmdLast, uint uFlags);

    [PreserveSig] int InvokeCommand(ref CMINVOKECOMMANDINFOEX pici);

    [PreserveSig] int GetCommandString(UIntPtr idCmd, uint uType, IntPtr pReserved, IntPtr pszName, uint cchMax);
}

[ComImport, Guid("000214F4-0000-0000-C000-000000000046"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IContextMenu2
{
    [PreserveSig] int QueryContextMenu(IntPtr hmenu, uint indexMenu, uint idCmdFirst, uint idCmdLast, uint uFlags);

    [PreserveSig] int InvokeCommand(ref CMINVOKECOMMANDINFOEX pici);

    [PreserveSig] int GetCommandString(UIntPtr idCmd, uint uType, IntPtr pReserved, IntPtr pszName, uint cchMax);

    [PreserveSig] int HandleMenuMsg(uint uMsg, IntPtr wParam, IntPtr lParam);
}

[ComImport, Guid("BCFCE0A0-EC17-11D0-8D10-00A0C90F2719"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IContextMenu3
{
    [PreserveSig] int QueryContextMenu(IntPtr hmenu, uint indexMenu, uint idCmdFirst, uint idCmdLast, uint uFlags);

    [PreserveSig] int InvokeCommand(ref CMINVOKECOMMANDINFOEX pici);

    [PreserveSig] int GetCommandString(UIntPtr idCmd, uint uType, IntPtr pReserved, IntPtr pszName, uint cchMax);

    [PreserveSig] int HandleMenuMsg(uint uMsg, IntPtr wParam, IntPtr lParam);

    [PreserveSig] int HandleMenuMsg2(uint uMsg, IntPtr wParam, IntPtr lParam, out IntPtr plResult);
}
