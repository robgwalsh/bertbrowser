using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Interop;

/// <summary>
/// Asking an already-open handle which file on disk it actually refers to.
/// </summary>
/// <remarks>
/// <para>
/// NTFS lets one file carry several names. A hardlinked file appears once per name in the MFT — same
/// size, same bytes — so a duplicate finder that only compared content would report every one of
/// them as a copy worth deleting, when deleting one frees nothing at all.
/// <c>C:\Windows\WinSxS</c> is built almost entirely this way and would otherwise dominate any
/// whole-PC result.
/// </para>
/// <para>
/// The volume serial plus the file index is the identity that settles it, and there is no managed
/// API for either — <c>FileInfo.LinkTarget</c> answers about symlinks, not hardlinks. It costs one
/// call on a handle the hasher has open anyway, and only when <c>nNumberOfLinks</c> says there is
/// more than one name to worry about.
/// </para>
/// </remarks>
internal static class FileIdentityNative
{
    [StructLayout(LayoutKind.Sequential)]
    internal struct ByHandleFileInformation
    {
        internal uint FileAttributes;
        internal uint CreationTimeLow;
        internal uint CreationTimeHigh;
        internal uint LastAccessTimeLow;
        internal uint LastAccessTimeHigh;
        internal uint LastWriteTimeLow;
        internal uint LastWriteTimeHigh;
        internal uint VolumeSerialNumber;
        internal uint FileSizeHigh;
        internal uint FileSizeLow;
        internal uint NumberOfLinks;
        internal uint FileIndexHigh;
        internal uint FileIndexLow;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle hFile, out ByHandleFileInformation lpFileInformation);

    /// <summary>
    /// The number of names this file answers to, and — only when that is more than one — the
    /// identity that tells its names apart from a genuine copy.
    /// </summary>
    /// <returns>
    /// False when the call fails, which is not an error worth surfacing: a filesystem that will not
    /// answer simply gets the safe reading, one link and no identity, and the file is then treated
    /// as its own thing. Under-collapsing shows a real file twice; over-collapsing would hide a
    /// duplicate the user could have removed.
    /// </returns>
    internal static bool TryRead(SafeFileHandle handle, out uint links, out (uint Volume, ulong Index) identity)
    {
        links = 1;
        identity = default;

        if (!GetFileInformationByHandle(handle, out var info)) return false;

        links = info.NumberOfLinks;
        identity = (info.VolumeSerialNumber, ((ulong)info.FileIndexHigh << 32) | info.FileIndexLow);
        return true;
    }
}
