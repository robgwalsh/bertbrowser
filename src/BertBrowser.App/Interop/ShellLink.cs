using System.IO;
using System.Runtime.InteropServices;
using BertBrowser.Core.Services.Shortcuts;

namespace BertBrowser.App.Interop;

/// <summary>
/// Writes a <c>.lnk</c> through the shell's own <c>IShellLink</c>, which is the only way to produce
/// one Explorer will read back — the format is undocumented and hand-rolling it is how you get a
/// shortcut that works until someone opens its properties.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing here decides anything.</b> Which links to write, what to call them and what to do when
/// a name is taken all live in <c>ShortcutPlanner</c>/<c>ShortcutExecutor</c>, in Core, where they
/// are tested without COM. This is the seam and no more, exactly as <c>IFileCopier</c> is for the
/// copy loop.
/// </para>
/// <para>
/// The working directory is set to the target's parent folder. Explorer does the same, and without
/// it a shortcut to a program starts it in whatever folder the shell happened to be in — which for
/// anything that writes beside itself is a real difference, not a cosmetic one.
/// </para>
/// </remarks>
internal sealed class ShellLink : IShortcutWriter
{
    public void Write(string linkPath, string targetPath)
    {
        // Apartment-threaded COM, and every caller is already on the UI thread — the shell classes
        // are not free-threaded and a link written off an MTA thread comes back as an E_NOINTERFACE
        // that reads like a missing registration.
        var link = (IShellLinkW)new ShellLinkCoClass();
        try
        {
            link.SetPath(targetPath);

            // A shortcut to a drive root has no parent to start in; anything else does.
            if (Path.GetDirectoryName(targetPath) is { Length: > 0 } parent)
                link.SetWorkingDirectory(parent);

            // fRemember: false — this is a new file, not a re-save of one the shell is tracking.
            ((IPersistFile)link).Save(linkPath, fRemember: false);
        }
        finally
        {
            Marshal.ReleaseComObject(link);
        }
    }

    /// <summary>CLSID_ShellLink. Deliberately not <c>sealed</c>: the cast to
    /// <see cref="IShellLinkW"/> is what the object is created for, and the compiler refuses that
    /// conversion on a sealed type that does not declare the interface.</summary>
    [ComImport]
    [Guid("00021401-0000-0000-C000-000000000046")]
    private class ShellLinkCoClass
    {
    }

    /// <summary>
    /// Only the members this app calls are declared with real signatures; the rest are placeholders
    /// holding their slots in the vtable, which is what makes the layout correct. Reordering or
    /// removing one would silently call the wrong method.
    /// </summary>
    [ComImport]
    [Guid("000214F9-0000-0000-C000-000000000046")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLinkW
    {
        void GetPath(
            [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder file,
            int maxPath,
            IntPtr findData,
            uint flags);

        void GetIDList(out IntPtr idList);

        void SetIDList(IntPtr idList);

        void GetDescription(
            [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder name, int maxName);

        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string name);

        void GetWorkingDirectory(
            [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder directory, int maxPath);

        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);

        void GetArguments(
            [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder arguments, int maxArguments);

        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string arguments);

        void GetHotkey(out short hotkey);

        void SetHotkey(short hotkey);

        void GetShowCmd(out int showCmd);

        void SetShowCmd(int showCmd);

        void GetIconLocation(
            [MarshalAs(UnmanagedType.LPWStr)] System.Text.StringBuilder iconPath,
            int iconPathLength,
            out int iconIndex);

        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string iconPath, int iconIndex);

        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pathRelative, uint reserved);

        void Resolve(IntPtr hwnd, uint flags);

        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string file);
    }

    [ComImport]
    [Guid("0000010b-0000-0000-C000-000000000046")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPersistFile
    {
        void GetClassID(out Guid classId);

        [PreserveSig]
        int IsDirty();

        void Load([MarshalAs(UnmanagedType.LPWStr)] string fileName, uint mode);

        void Save([MarshalAs(UnmanagedType.LPWStr)] string fileName, [MarshalAs(UnmanagedType.Bool)] bool fRemember);

        void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string fileName);

        void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string fileName);
    }
}
