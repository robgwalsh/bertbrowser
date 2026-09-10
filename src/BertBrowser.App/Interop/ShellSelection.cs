using System.Runtime.InteropServices;

namespace BertBrowser.App.Interop;

/// <summary>
/// What a context-menu handler is initialised with: the folder's PIDL and, for a selection, a data
/// object naming the items — the same two things Explorer hands it, so a handler that reads
/// <c>CF_HDROP</c> or the shell ID list finds what it expects.
/// </summary>
/// <remarks>
/// The data object is kept as a raw <c>IUnknown</c> pointer rather than an RCW: the only thing done
/// with it is handing it to <c>IShellExtInit::Initialize</c>, which takes a pointer, and a pointer
/// released once in <see cref="Dispose"/> has exactly one lifetime to reason about.
/// </remarks>
internal sealed class ShellSelection : IDisposable
{
    private static readonly Guid BhidDataObject = new("B8C0BD9F-ED24-455C-83E6-D5390C4FE8C4");
    private static readonly Guid IidIDataObject = new("0000010E-0000-0000-C000-000000000046");

    private readonly IntPtr[] _itemPidls;
    private bool _disposed;

    private ShellSelection(IntPtr folderPidl, IntPtr[] itemPidls, IntPtr dataObject)
    {
        FolderPidl = folderPidl;
        _itemPidls = itemPidls;
        DataObject = dataObject;
    }

    public IntPtr FolderPidl { get; }

    /// <summary>An <c>IDataObject</c>, or zero for a folder background.</summary>
    public IntPtr DataObject { get; }

    /// <summary>Null when the folder cannot be parsed, or when items were asked for and none of
    /// them could be — a handler initialised with an empty selection would act on nothing.</summary>
    public static ShellSelection? Create(string folder, IReadOnlyList<string> itemPaths)
    {
        var folderPidl = Parse(folder);
        if (folderPidl == IntPtr.Zero) return null;

        var pidls = new List<IntPtr>(itemPaths.Count);
        foreach (var path in itemPaths)
        {
            // A row deleted since it was listed is simply not part of the selection.
            var pidl = Parse(path);
            if (pidl != IntPtr.Zero) pidls.Add(pidl);
        }

        if (itemPaths.Count > 0 && pidls.Count == 0)
        {
            ILFree(folderPidl);
            return null;
        }

        var dataObject = IntPtr.Zero;
        if (pidls.Count > 0)
        {
            dataObject = DataObjectFor(pidls);
            if (dataObject == IntPtr.Zero)
            {
                foreach (var pidl in pidls) ILFree(pidl);
                ILFree(folderPidl);
                return null;
            }
        }

        return new ShellSelection(folderPidl, pidls.ToArray(), dataObject);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (DataObject != IntPtr.Zero) Marshal.Release(DataObject);
        foreach (var pidl in _itemPidls) ILFree(pidl);
        ILFree(FolderPidl);
    }

    private static IntPtr Parse(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return IntPtr.Zero;

        return SHParseDisplayName(path, IntPtr.Zero, out var pidl, 0, out _) == 0 ? pidl : IntPtr.Zero;
    }

    private static IntPtr DataObjectFor(List<IntPtr> pidls)
    {
        var array = pidls.ToArray();
        if (SHCreateShellItemArrayFromIDLists((uint)array.Length, array, out var items) != 0 || items is null)
            return IntPtr.Zero;

        try
        {
            var bhid = BhidDataObject;
            var iid = IidIDataObject;
            return items.BindToHandler(IntPtr.Zero, ref bhid, ref iid, out var dataObject) == 0
                ? dataObject
                : IntPtr.Zero;
        }
        finally
        {
            Marshal.FinalReleaseComObject(items);
        }
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int SHParseDisplayName(
        string pszName, IntPtr pbc, out IntPtr ppidl, uint sfgaoIn, out uint psfgaoOut);

    [DllImport("shell32.dll", ExactSpelling = true)]
    private static extern int SHCreateShellItemArrayFromIDLists(
        uint cidl, [In] IntPtr[] rgpidl, [MarshalAs(UnmanagedType.Interface)] out IShellItemArray? ppsiItemArray);

    [DllImport("shell32.dll", ExactSpelling = true)]
    private static extern void ILFree(IntPtr pidl);
}

/// <summary>Only <c>BindToHandler</c> is called, and it is the first slot; the rest are declared
/// so the vtable reads as the header does.</summary>
[ComImport, Guid("b63ea76d-1f85-456f-a19c-48159efa858b"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IShellItemArray
{
    [PreserveSig] int BindToHandler(IntPtr pbc, ref Guid bhid, ref Guid riid, out IntPtr ppvOut);

    [PreserveSig] int GetPropertyStore(int flags, ref Guid riid, out IntPtr ppv);

    [PreserveSig] int GetPropertyDescriptionList(IntPtr keyType, ref Guid riid, out IntPtr ppv);

    [PreserveSig] int GetAttributes(int attribFlags, uint sfgaoMask, out uint psfgaoAttribs);

    [PreserveSig] int GetCount(out uint pdwNumItems);

    [PreserveSig] int GetItemAt(uint dwIndex, out IntPtr ppsi);

    [PreserveSig] int EnumItems(out IntPtr ppenumShellItems);
}
