using System.Runtime.InteropServices;

namespace BertBrowser.App.Interop;

/// <summary>Explorer-style "logical" string comparison (file2 &lt; file10).</summary>
public sealed class NaturalStringComparer : IComparer<string?>
{
    public static readonly NaturalStringComparer Instance = new();

    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode)]
    private static extern int StrCmpLogicalW(string psz1, string psz2);

    public int Compare(string? x, string? y)
    {
        if (x is null) return y is null ? 0 : -1;
        if (y is null) return 1;
        return StrCmpLogicalW(x, y);
    }
}
