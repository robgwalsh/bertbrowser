using System.Runtime.InteropServices;

namespace BertBrowser.App.Interop;

/// <summary>
/// The user folders "Go to" offers that <see cref="Environment.SpecialFolder"/> has no name for.
/// </summary>
/// <remarks>
/// Downloads is the one that matters and the one .NET cannot find: it is a known folder, not a
/// CSIDL, and people move it to another drive often enough that guessing
/// <c>%USERPROFILE%\Downloads</c> would open a folder that is not theirs. The guess is still the
/// fallback, because a wrong folder that exists beats nothing at all.
/// </remarks>
internal static class KnownFolders
{
    private static readonly Guid DownloadsId = new("374DE290-123F-4565-9164-39C4925E467B");

    public static string Downloads =>
        Resolve(DownloadsId)
        ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");

    private static string? Resolve(Guid id)
    {
        var buffer = IntPtr.Zero;
        try
        {
            return SHGetKnownFolderPath(id, 0, IntPtr.Zero, out buffer) == 0
                ? Marshal.PtrToStringUni(buffer)
                : null;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            return null;
        }
        finally
        {
            if (buffer != IntPtr.Zero) Marshal.FreeCoTaskMem(buffer);
        }
    }

    [DllImport("shell32.dll", ExactSpelling = true)]
    private static extern int SHGetKnownFolderPath(
        [MarshalAs(UnmanagedType.LPStruct)] Guid id, uint flags, IntPtr token, out IntPtr path);
}
