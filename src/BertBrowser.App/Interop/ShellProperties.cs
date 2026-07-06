using System.Runtime.InteropServices;
using System.Text;

namespace BertBrowser.App.Interop;

/// <summary>A single shell property: localized display name + display-formatted value.</summary>
public sealed record ShellProperty(string Name, string Value);

/// <summary>
/// Reads everything the file's registered property handlers expose (EXIF, ID3,
/// media, document properties, …) via the Windows Shell Property System — the
/// same source as Explorer's Details tab.
/// </summary>
public static class ShellProperties
{
    /// <summary>Blocking (property handlers read file content); call from a background thread.</summary>
    public static IReadOnlyList<ShellProperty> Read(string path)
    {
        try
        {
            return ReadCore(path);
        }
        catch
        {
            // A misbehaving third-party property handler must never take the app down.
            return Array.Empty<ShellProperty>();
        }
    }

    private static IReadOnlyList<ShellProperty> ReadCore(string path)
    {
        var iid = IID_IPropertyStore;
        if (SHGetPropertyStoreFromParsingName(path, IntPtr.Zero, GPS_BESTEFFORT | GPS_OPENSLOWITEM, ref iid, out var store) != 0)
            return Array.Empty<ShellProperty>();

        try
        {
            store.GetCount(out var count);
            var byName = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);

            for (uint i = 0; i < count; i++)
            {
                try
                {
                    store.GetAt(i, out var key);

                    // Properties without a registered description are internal plumbing
                    // keys; Explorer hides them too.
                    var name = GetDisplayName(ref key);
                    if (string.IsNullOrWhiteSpace(name) || byName.ContainsKey(name))
                        continue;

                    var pv = default(PROPVARIANT);
                    try
                    {
                        store.GetValue(ref key, out pv);
                        if (pv.vt is VT_EMPTY or VT_NULL)
                            continue;

                        var text = new StringBuilder(1024);
                        if (PSFormatForDisplay(ref key, ref pv, PDFF_DEFAULT, text, (uint)text.Capacity) < 0)
                            continue;

                        var value = text.ToString().Trim();
                        if (value.Length > 0)
                            byName[name] = value;
                    }
                    finally
                    {
                        PropVariantClear(ref pv);
                    }
                }
                catch (COMException)
                {
                    // One broken property must not hide the rest.
                }
            }

            return byName
                .Select(kv => new ShellProperty(kv.Key, kv.Value))
                .OrderBy(p => p.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();
        }
        finally
        {
            Marshal.ReleaseComObject(store);
        }
    }

    private static string? GetDisplayName(ref PROPERTYKEY key)
    {
        var iid = IID_IPropertyDescription;
        if (PSGetPropertyDescription(ref key, ref iid, out var desc) != 0)
            return null;

        try
        {
            desc.GetDisplayName(out var ptr);
            if (ptr == IntPtr.Zero)
                return null;
            try
            {
                return Marshal.PtrToStringUni(ptr);
            }
            finally
            {
                Marshal.FreeCoTaskMem(ptr);
            }
        }
        catch (COMException)
        {
            return null; // no display name registered for this key
        }
        finally
        {
            Marshal.ReleaseComObject(desc);
        }
    }

    // --- interop ---

    private const uint GPS_BESTEFFORT = 0x40;   // degrade gracefully on per-handler failure
    private const uint GPS_OPENSLOWITEM = 0x10; // open the file so content handlers (EXIF, ID3, …) run
    private const int PDFF_DEFAULT = 0;
    private const ushort VT_EMPTY = 0;
    private const ushort VT_NULL = 1;

    private static readonly Guid IID_IPropertyStore = new("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99");
    private static readonly Guid IID_IPropertyDescription = new("6f79d558-3e96-4549-a1d1-7d75d2288814");

    [StructLayout(LayoutKind.Sequential)]
    private struct PROPERTYKEY
    {
        public Guid fmtid;
        public uint pid;
    }

    /// <summary>Opaque blob: only ever passed by ref to PSFormatForDisplay / PropVariantClear;
    /// vt is inspected solely to skip empty values.</summary>
    [StructLayout(LayoutKind.Sequential)]
    private struct PROPVARIANT
    {
        public ushort vt;
        public ushort r1, r2, r3;
        public IntPtr p1, p2;
    }

    [ComImport, Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        void GetCount(out uint cProps);
        void GetAt(uint iProp, out PROPERTYKEY pkey);
        void GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
        void SetValue(ref PROPERTYKEY key, ref PROPVARIANT pv);
        void Commit();
    }

    // Vtable order matters: all slots up to GetDisplayName must be declared.
    [ComImport, Guid("6f79d558-3e96-4549-a1d1-7d75d2288814"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyDescription
    {
        void GetPropertyKey(out PROPERTYKEY pkey);
        void GetCanonicalName(out IntPtr ppszName);
        void GetPropertyType(out ushort vartype);
        void GetDisplayName(out IntPtr ppszName);
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHGetPropertyStoreFromParsingName(
        string pszPath, IntPtr pbc, uint flags, ref Guid riid, out IPropertyStore store);

    [DllImport("propsys.dll")]
    private static extern int PSGetPropertyDescription(
        ref PROPERTYKEY key, ref Guid riid, out IPropertyDescription desc);

    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    private static extern int PSFormatForDisplay(
        ref PROPERTYKEY key, ref PROPVARIANT pv, int pdffFlags, StringBuilder text, uint cchText);

    [DllImport("ole32.dll")]
    private static extern int PropVariantClear(ref PROPVARIANT pv);
}
