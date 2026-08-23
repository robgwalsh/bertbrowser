using System.Runtime.InteropServices;
using System.Text;
using BertBrowser.Core.Services.NewItem;
using Microsoft.Win32;

namespace BertBrowser.App.Interop;

/// <summary>
/// Reads the <c>ShellNew</c> keys Windows builds Explorer's New menu from.
/// </summary>
/// <remarks>
/// <para>
/// <b>Read-only, and that is the whole contract.</b> Every key is opened without write access and
/// nothing here creates, sets or deletes a value. Registering new-file types with the shell would
/// change Explorer's own New menu machine-wide, and removing an entry another installer put in
/// HKLM would need an administrator token this app deliberately does not hold — so BertBrowser
/// keeps its own list and only ever *reads* this one to populate it.
/// </para>
/// <para>
/// Nothing but raw values leaves here: which of them are worth offering, what a type is called and
/// where its template is are all decided by <see cref="ShellNewImport"/>, in Core, where they can be
/// tested by a project that cannot open a registry key.
/// </para>
/// </remarks>
internal static class ShellNewRegistry
{
    /// <summary>Every extension with a usable ShellNew entry. Never throws: a key that cannot be
    /// read is one fewer type on the menu, not a failed import.</summary>
    public static IReadOnlyList<ShellNewEntry> Read()
    {
        var entries = new List<ShellNewEntry>();

        try
        {
            using var classes = Registry.ClassesRoot;
            foreach (var name in classes.GetSubKeyNames())
            {
                if (name.Length < 2 || name[0] != '.') continue;

                try
                {
                    if (ReadExtension(classes, name) is { } entry) entries.Add(entry);
                }
                catch (Exception ex) when (IsRegistryFailure(ex))
                {
                }
            }
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
        }

        return entries;
    }

    private static ShellNewEntry? ReadExtension(RegistryKey classes, string extension)
    {
        using var extensionKey = classes.OpenSubKey(extension, writable: false);
        if (extensionKey is null) return null;

        // ".docx\ShellNew" is the common shape, but Office and others hang it off the ProgID
        // subkey instead (".docx\Word.Document.12\ShellNew"), so look one level down too.
        using var shellNew = FindShellNew(extensionKey);
        if (shellNew is null) return null;

        var kind = KindOf(shellNew, out var fileName, out var data);
        if (kind is null) return null;

        return new ShellNewEntry(extension, LabelOf(classes, extensionKey), kind.Value, fileName, data);
    }

    private static RegistryKey? FindShellNew(RegistryKey extensionKey)
    {
        if (extensionKey.OpenSubKey("ShellNew", writable: false) is { } direct) return direct;

        foreach (var child in extensionKey.GetSubKeyNames())
        {
            try
            {
                using var childKey = extensionKey.OpenSubKey(child, writable: false);
                if (childKey?.OpenSubKey("ShellNew", writable: false) is { } nested) return nested;
            }
            catch (Exception ex) when (IsRegistryFailure(ex))
            {
            }
        }

        return null;
    }

    /// <summary>Which of the four value shapes this key uses. Command is reported rather than
    /// hidden, so the decision to drop it stays in one place in Core.</summary>
    private static ShellNewKind? KindOf(RegistryKey shellNew, out string? fileName, out byte[]? data)
    {
        fileName = null;
        data = null;
        var names = shellNew.GetValueNames();

        if (names.Contains("Command", StringComparer.OrdinalIgnoreCase))
            return ShellNewKind.Command;

        if (names.Contains("FileName", StringComparer.OrdinalIgnoreCase))
        {
            fileName = shellNew.GetValue("FileName") as string;
            return fileName is { Length: > 0 } ? ShellNewKind.FileName : null;
        }

        if (names.Contains("Data", StringComparer.OrdinalIgnoreCase))
        {
            data = shellNew.GetValue("Data") as byte[];
            return data is { Length: > 0 } ? ShellNewKind.Data : null;
        }

        return names.Contains("NullFile", StringComparer.OrdinalIgnoreCase)
            ? ShellNewKind.NullFile
            : null;
    }

    /// <summary>The friendly type name, which may still be an unresolved "@dll,-id" reference —
    /// resolving it is <see cref="ShellNewImport"/>'s business, through <see cref="LoadIndirectString"/>.</summary>
    private static string? LabelOf(RegistryKey classes, RegistryKey extensionKey)
    {
        // An extension key's default value is its ProgID, and the name worth showing hangs off
        // that rather than off the extension itself.
        if (extensionKey.GetValue(null) is not string progId || progId.Length == 0) return null;

        try
        {
            using var progIdKey = classes.OpenSubKey(progId, writable: false);
            if (progIdKey is null) return null;

            return progIdKey.GetValue("FriendlyTypeName") as string
                ?? progIdKey.GetValue(null) as string;
        }
        catch (Exception ex) when (IsRegistryFailure(ex))
        {
            return null;
        }
    }

    /// <summary>Resolves an "@%SystemRoot%\system32\notepad.exe,-469" style reference to the string
    /// it names, or null when it cannot be loaded.</summary>
    public static string? LoadIndirectString(string reference)
    {
        var buffer = new StringBuilder(1024);
        return SHLoadIndirectString(reference, buffer, buffer.Capacity, nint.Zero) == 0
            ? buffer.ToString()
            : null;
    }

    /// <summary>Where a bare ShellNew template name is looked for.</summary>
    public static IReadOnlyList<string> TemplateRoots()
    {
        var roots = new List<string>();

        try
        {
            var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
            if (appData.Length > 0)
                roots.Add(Path.Combine(appData, "Microsoft", "Windows", "Templates"));

            var windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            if (windows.Length > 0) roots.Add(Path.Combine(windows, "ShellNew"));
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException)
        {
        }

        return roots;
    }

    private static bool IsRegistryFailure(Exception ex) =>
        ex is System.Security.SecurityException or UnauthorizedAccessException or IOException;

    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int SHLoadIndirectString(
        string source, StringBuilder outBuffer, int outBufferSize, nint reserved);
}
