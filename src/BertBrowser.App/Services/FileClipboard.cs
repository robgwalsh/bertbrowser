using System.Collections.Specialized;
using System.Windows;

namespace BertBrowser.App.Services;

/// <summary>
/// Explorer-compatible file clipboard: CF_HDROP for the paths plus the
/// "Preferred DropEffect" stream that distinguishes cut from copy, so
/// copy/cut/paste interoperates with Windows Explorer in both directions.
/// </summary>
public static class FileClipboard
{
    private const string DropEffectFormat = "Preferred DropEffect";

    public static void SetFiles(IReadOnlyCollection<string> paths, bool cut)
    {
        var list = new StringCollection();
        foreach (var path in paths)
            list.Add(path);

        var effect = (int)(cut ? DragDropEffects.Move : DragDropEffects.Copy);
        var data = new DataObject();
        data.SetFileDropList(list);
        data.SetData(DropEffectFormat, new MemoryStream(BitConverter.GetBytes(effect)));
        Clipboard.SetDataObject(data, copy: true);
    }

    public static bool HasFiles()
    {
        try
        {
            return Clipboard.ContainsFileDropList();
        }
        catch (System.Runtime.InteropServices.ExternalException)
        {
            return false; // clipboard momentarily locked by another process
        }
    }

    public static (IReadOnlyList<string> Paths, bool IsCut)? GetFiles()
    {
        if (!Clipboard.ContainsFileDropList()) return null;

        var paths = Clipboard.GetFileDropList().Cast<string?>()
            .Where(p => !string.IsNullOrEmpty(p))
            .Select(p => p!)
            .ToList();
        if (paths.Count == 0) return null;

        var cut = false;
        if (Clipboard.GetData(DropEffectFormat) is MemoryStream stream)
        {
            var bytes = new byte[4];
            if (stream.Read(bytes, 0, 4) == 4)
                cut = ((DragDropEffects)BitConverter.ToInt32(bytes, 0)).HasFlag(DragDropEffects.Move);
        }
        return (paths, cut);
    }

    public static void Clear() => Clipboard.Clear();
}
