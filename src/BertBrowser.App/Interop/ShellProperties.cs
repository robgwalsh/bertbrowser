using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Preview;
using System.Text;

namespace BertBrowser.App.Interop;

/// <summary>A single shell property: localized display name + display-formatted value, plus the
/// invariant canonical key ("System.Image.Dimensions") for anything that needs to <em>choose</em>
/// properties rather than list them — see <c>PreviewMetadata</c>, which cannot match on
/// <paramref name="Name"/> because that is whatever language Windows is in.</summary>
public sealed record ShellProperty(string Name, string Value, string Canonical);

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
            var byName = new Dictionary<string, (string Value, string Canonical)>(StringComparer.OrdinalIgnoreCase);

            for (uint i = 0; i < count; i++)
            {
                try
                {
                    store.GetAt(i, out var key);

                    // Properties without a registered description are internal plumbing
                    // keys; Explorer hides them too.
                    var (name, canonical) = Describe(ref key);
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
                            byName[name] = (value, canonical);
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
                .Select(kv => new ShellProperty(kv.Key, kv.Value.Value, kv.Value.Canonical))
                .OrderBy(p => p.Name, StringComparer.CurrentCultureIgnoreCase)
                .ToList();
        }
        finally
        {
            Marshal.ReleaseComObject(store);
        }
    }

    // --- Keyed reads, for columns ---
    //
    // Read() above enumerates every property a file has, and does a PSGetPropertyDescription COM
    // activation per property per file to learn their names — right for the Properties dialog,
    // which shows all of them, and hopeless per row. Everything below asks for named keys instead,
    // so a description lookup becomes a per-*column* cost paid once instead of a per-file one.

    /// <summary>Canonical name to key. A registered name's key never changes, so this is resolved
    /// once per column for the life of the process rather than per file.</summary>
    private static readonly ConcurrentDictionary<string, PROPERTYKEY?> KeyCache =
        new(StringComparer.OrdinalIgnoreCase);

    private static PROPERTYKEY? KeyFor(string canonical) =>
        KeyCache.GetOrAdd(canonical, name =>
            PSGetPropertyKeyFromName(name, out var key) == 0 ? key : null);

    /// <summary>The localised label for a canonical name, for a column header. Null when this
    /// machine has no description registered for it.</summary>
    public static string? DisplayNameFor(string canonical)
    {
        try
        {
            if (KeyFor(canonical) is not { } key) return null;
            return Describe(ref key).Display;
        }
        catch (Exception)
        {
            return null; // a third-party handler must never take the app down — as in Read
        }
    }

    /// <summary>
    /// Reads just the named properties of one file.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Blocking, and it opens the file</b> (<c>GPS_OPENSLOWITEM</c>, so EXIF and ID3 handlers
    /// actually run). Call it from a background thread, under a concurrency bound.
    /// </para>
    /// <para>
    /// <paramref name="attributes"/> is taken rather than probed, and the refusals below are here
    /// rather than at the call site so they cannot be forgotten by a second caller. The first is
    /// the one that matters: opening a cloud placeholder makes the provider fetch it, so a column
    /// scrolled past a synced photo folder would quietly download the lot.
    /// </para>
    /// </remarks>
    public static IReadOnlyDictionary<string, ColumnValue> ReadValues(
        string path, FileAttributes attributes, IReadOnlyList<string> canonicals)
    {
        if (canonicals.Count == 0) return EmptyValues;

        // The refusals are a Core rule, not a condition written out here, so the one that matters —
        // never opening a cloud placeholder — is somewhere a test can reach it.
        if (!MetadataReadRules.MayRead(attributes)) return EmptyValues;

        try
        {
            return ReadValuesCore(path, canonicals);
        }
        catch (Exception)
        {
            return EmptyValues;
        }
    }

    private static readonly Dictionary<string, ColumnValue> EmptyValues = new();

    private static IReadOnlyDictionary<string, ColumnValue> ReadValuesCore(
        string path, IReadOnlyList<string> canonicals)
    {
        var iid = IID_IPropertyStore;
        if (SHGetPropertyStoreFromParsingName(
                path, IntPtr.Zero, GPS_BESTEFFORT | GPS_OPENSLOWITEM, ref iid, out var store) != 0)
            return EmptyValues;

        var values = new Dictionary<string, ColumnValue>(canonicals.Count, StringComparer.OrdinalIgnoreCase);
        try
        {
            foreach (var canonical in canonicals)
            {
                if (KeyFor(canonical) is not { } key) continue;

                var pv = default(PROPVARIANT);
                try
                {
                    store.GetValue(ref key, out pv);
                    if (pv.vt is VT_EMPTY or VT_NULL) continue;

                    var text = new StringBuilder(1024);
                    if (PSFormatForDisplay(ref key, ref pv, PDFF_DEFAULT, text, (uint)text.Capacity) != 0)
                        continue;

                    var display = text.ToString().Trim();
                    if (display.Length == 0) continue;

                    // Capped because, unlike Read's, this string is *kept*: a Comments or Keywords
                    // field can fill the whole buffer, and a cache of them would not be small.
                    if (display.Length > MaxDisplayLength) display = display[..MaxDisplayLength];

                    values[canonical] = new ColumnValue(display, NumberOf(pv), DateOf(pv));
                }
                catch (COMException)
                {
                    // One broken property must not hide the rest — as in Read.
                }
                finally
                {
                    PropVariantClear(ref pv);
                }
            }
        }
        finally
        {
            Marshal.ReleaseComObject(store);
        }
        return values;
    }

    /// <summary>Every property this machine has a description for, for the "More columns…" picker.
    /// Ordered and filtered by the caller; anything without a display name is plumbing.</summary>
    public static IReadOnlyList<(string Canonical, string Display)> EnumerateDescriptions()
    {
        var found = new List<(string, string)>(512);
        try
        {
            var iid = IID_IPropertyDescriptionList;
            if (PSEnumeratePropertyDescriptions(PDEF_ALL, ref iid, out var list) != 0) return found;

            try
            {
                list.GetCount(out var count);
                var descIid = IID_IPropertyDescription;
                for (uint i = 0; i < count; i++)
                {
                    IPropertyDescription? desc = null;
                    try
                    {
                        list.GetAt(i, ref descIid, out desc);
                        var canonical = TakeString(() => { desc.GetCanonicalName(out var p); return p; });
                        var display = TakeString(() => { desc.GetDisplayName(out var p); return p; });
                        if (canonical is { Length: > 0 } && display is { Length: > 0 })
                            found.Add((canonical, display));
                    }
                    catch (COMException)
                    {
                        // Skip the one description; the rest of the system is still worth listing.
                    }
                    finally
                    {
                        if (desc is not null) Marshal.ReleaseComObject(desc);
                    }
                }
            }
            finally
            {
                Marshal.ReleaseComObject(list);
            }
        }
        catch (Exception)
        {
            // An unreadable property system means an empty picker, never a crash.
        }
        return found;
    }

    // The variant types worth unwrapping, and no more. On x64 the PROPVARIANT union starts at
    // offset 8, which is p1 — so the 8-byte types are read straight out of it and the 4-byte ones
    // out of its low half.
    private static double? NumberOf(PROPVARIANT pv) => (pv.vt & VT_TYPEMASK) switch
    {
        _ when IsIndirect(pv) => null,
        VT_I4 or VT_INT => (int)(pv.p1.ToInt64() & 0xFFFFFFFF),
        VT_UI4 or VT_UINT => (uint)(pv.p1.ToInt64() & 0xFFFFFFFF),
        VT_I8 => pv.p1.ToInt64(),
        VT_UI8 => (double)(ulong)pv.p1.ToInt64(),
        VT_R8 => BitConverter.Int64BitsToDouble(pv.p1.ToInt64()),
        VT_R4 => BitConverter.Int32BitsToSingle((int)(pv.p1.ToInt64() & 0xFFFFFFFF)),
        _ => null,
    };

    private static DateTime? DateOf(PROPVARIANT pv)
    {
        if (IsIndirect(pv) || (pv.vt & VT_TYPEMASK) != VT_FILETIME) return null;
        try
        {
            return DateTime.FromFileTimeUtc(pv.p1.ToInt64());
        }
        catch (ArgumentOutOfRangeException)
        {
            return null; // a handler can hand back a nonsense FILETIME
        }
    }

    /// <summary>
    /// Whether the variant holds a pointer to something rather than the value itself.
    /// </summary>
    /// <remarks>
    /// This is the check that turns a wrong read into garbage rather than a wrong number.
    /// <c>System.Music.Artist</c> and <c>System.Keywords</c> are genuinely
    /// <c>VT_VECTOR|VT_LPWSTR</c>, and reading <c>p1</c> as a double there would format a pointer
    /// address as the column's sort key.
    /// </remarks>
    private static bool IsIndirect(PROPVARIANT pv) =>
        (pv.vt & (VT_VECTOR | VT_ARRAY | VT_BYREF)) != 0;

    /// <summary>The property's localized label and its invariant canonical name, from one
    /// description lookup. A missing display name means the key is plumbing and is dropped by the
    /// caller; a missing canonical name only costs the ability to select on it.</summary>
    private static (string? Display, string Canonical) Describe(ref PROPERTYKEY key)
    {
        var iid = IID_IPropertyDescription;
        if (PSGetPropertyDescription(ref key, ref iid, out var desc) != 0)
            return (null, "");

        try
        {
            return (TakeString(() => { desc.GetDisplayName(out var p); return p; }),
                    TakeString(() => { desc.GetCanonicalName(out var p); return p; }) ?? "");
        }
        catch (COMException)
        {
            return (null, ""); // no description registered for this key
        }
        finally
        {
            Marshal.ReleaseComObject(desc);
        }
    }

    /// <summary>Reads a shell-allocated string and frees it, however that turns out.</summary>
    private static string? TakeString(Func<IntPtr> get)
    {
        var ptr = IntPtr.Zero;
        try
        {
            ptr = get();
            return ptr == IntPtr.Zero ? null : Marshal.PtrToStringUni(ptr);
        }
        catch (COMException)
        {
            return null;
        }
        finally
        {
            if (ptr != IntPtr.Zero) Marshal.FreeCoTaskMem(ptr);
        }
    }

    // --- interop ---

    private const uint GPS_BESTEFFORT = 0x40;   // degrade gracefully on per-handler failure
    private const uint GPS_OPENSLOWITEM = 0x10; // open the file so content handlers (EXIF, ID3, …) run
    private const int PDFF_DEFAULT = 0;
    private const ushort VT_EMPTY = 0;
    private const ushort VT_NULL = 1;
    private const ushort VT_I4 = 3;
    private const ushort VT_R4 = 4;
    private const ushort VT_R8 = 5;
    private const ushort VT_I8 = 20;
    private const ushort VT_UI8 = 21;
    private const ushort VT_INT = 22;
    private const ushort VT_UINT = 23;
    private const ushort VT_UI4 = 19;
    private const ushort VT_FILETIME = 64;
    private const ushort VT_TYPEMASK = 0x0FFF;
    private const ushort VT_VECTOR = 0x1000;
    private const ushort VT_ARRAY = 0x2000;
    private const ushort VT_BYREF = 0x4000;

    /// <summary>PDEF_ALL — every registered description. The picker does its own filtering.</summary>
    private const int PDEF_ALL = 0;

    /// <summary>A kept display string is capped here; see <see cref="ReadValuesCore"/>.</summary>
    private const int MaxDisplayLength = 512;

    private static readonly Guid IID_IPropertyStore = new("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99");
    private static readonly Guid IID_IPropertyDescription = new("6f79d558-3e96-4549-a1d1-7d75d2288814");
    private static readonly Guid IID_IPropertyDescriptionList = new("1f9fc1d0-c39b-4b26-817f-011967d3440e");

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

    [ComImport, Guid("1f9fc1d0-c39b-4b26-817f-011967d3440e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyDescriptionList
    {
        void GetCount(out uint pcElem);
        void GetAt(uint iElem, ref Guid riid, out IPropertyDescription desc);
    }

    [DllImport("propsys.dll")]
    private static extern int PSGetPropertyDescription(
        ref PROPERTYKEY key, ref Guid riid, out IPropertyDescription desc);

    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    private static extern int PSGetPropertyKeyFromName(string pszName, out PROPERTYKEY pkey);

    [DllImport("propsys.dll")]
    private static extern int PSEnumeratePropertyDescriptions(
        int filterOn, ref Guid riid, out IPropertyDescriptionList list);

    [DllImport("propsys.dll", CharSet = CharSet.Unicode)]
    private static extern int PSFormatForDisplay(
        ref PROPERTYKEY key, ref PROPVARIANT pv, int pdffFlags, StringBuilder text, uint cchText);

    [DllImport("ole32.dll")]
    private static extern int PropVariantClear(ref PROPVARIANT pv);
}
