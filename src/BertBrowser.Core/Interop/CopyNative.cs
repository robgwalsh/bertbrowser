using System.Runtime.InteropServices;

namespace BertBrowser.Core.Interop;

/// <summary>
/// Thin P/Invoke layer for copying and moving a single file with byte-level progress and
/// interruption.
/// </summary>
/// <remarks>
/// <para>
/// <b>Why not a managed stream loop.</b> <c>File.Copy</c> is <c>CopyFile2</c> underneath, and going
/// through the OS is what preserves sparse-file handling, server-side copy on SMB, and the exact
/// attribute and timestamp semantics the rest of the transfer code already relies on. A hand-rolled
/// read/write loop would be slower on large files and network paths and would leave us
/// re-implementing those semantics by hand.
/// </para>
/// <para>
/// <b>Cancellation is the other half.</b> The progress routine returns
/// <see cref="ProgressCancel"/> to stop mid-file, and both calls then remove the partial
/// destination themselves and leave the source untouched — which is exactly the guarantee a
/// cancelled transfer has to make.
/// </para>
/// </remarks>
internal static class CopyNative
{
    // --- LPPROGRESS_ROUTINE return values ---
    internal const uint ProgressContinue = 0;
    internal const uint ProgressCancel = 1;

    // --- dwCopyFlags ---
    internal const uint CopyFileFailIfExists = 0x00000001;

    // --- dwFlags for MoveFileWithProgressW ---
    internal const uint MoveFileCopyAllowed = 0x00000002;
    internal const uint MoveFileWriteThrough = 0x00000008;

    // --- Error codes worth distinguishing ---
    internal const int ErrorRequestAborted = 1235;
    internal const int ErrorNotSameDevice = 17;

    /// <summary>
    /// The callback both entry points take. Only <paramref name="totalBytes"/> and
    /// <paramref name="transferred"/> are used here; the rest of the signature has to be present
    /// for the stack to line up.
    /// </summary>
    internal delegate uint ProgressRoutine(
        long totalBytes,
        long transferred,
        long streamSize,
        long streamTransferred,
        uint streamNumber,
        uint callbackReason,
        IntPtr sourceFile,
        IntPtr destinationFile,
        IntPtr data);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CopyFileExW(
        string lpExistingFileName,
        string lpNewFileName,
        ProgressRoutine? lpProgressRoutine,
        IntPtr lpData,
        ref int pbCancel,
        uint dwCopyFlags);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool MoveFileWithProgressW(
        string lpExistingFileName,
        string lpNewFileName,
        ProgressRoutine? lpProgressRoutine,
        IntPtr lpData,
        uint dwFlags);
}
