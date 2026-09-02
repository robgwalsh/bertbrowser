using System.ComponentModel;
using System.Runtime.InteropServices;
using BertBrowser.Core.Interop;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// Moving the bytes of one file, with progress and interruption.
/// </summary>
/// <remarks>
/// This is a seam for the same reason <see cref="ITransferProbe"/> is one: it is what lets
/// <c>TransferExecutorTests</c> drive a cancel that lands <em>in the middle of a file</em>
/// deterministically and in milliseconds, rather than by writing a multi-gigabyte fixture and
/// racing it.
/// </remarks>
public interface IFileCopier
{
    /// <summary>
    /// Copies one file, never overwriting. <paramref name="progress"/> is called with
    /// (bytes so far, bytes in total) as they land.
    /// </summary>
    /// <remarks>A cancelled copy throws <see cref="OperationCanceledException"/> and leaves no
    /// partial destination behind.</remarks>
    void Copy(string source, string destination, Action<long, long>? progress, CancellationToken ct);

    /// <summary>
    /// Moves one file, never overwriting. Within a volume this is a rename and reports nothing;
    /// across volumes it copies under the same progress and cancellation contract.
    /// </summary>
    /// <remarks>A cancelled move throws <see cref="OperationCanceledException"/>, leaves no partial
    /// destination, and leaves the source where it was.</remarks>
    void Move(string source, string destination, Action<long, long>? progress, CancellationToken ct);
}

/// <summary>
/// The real thing, over <c>CopyFileExW</c> and <c>MoveFileWithProgressW</c>.
/// </summary>
/// <remarks>
/// <para>
/// Both entry points do their own cleanup on a cancel — the partial destination is removed and the
/// source is untouched — which is the guarantee the transfer executor passes on to the user. The
/// destination is deleted defensively afterwards anyway, because "nothing half-written is left
/// where a finished file belongs" is worth more than one wasted <c>File.Exists</c>.
/// </para>
/// <para>
/// <c>MoveFileCopyAllowed</c> means the move spans volumes by itself, so unlike
/// <see cref="Directory.Move(string, string)"/> there is no <c>ERROR_NOT_SAME_DEVICE</c> fallback to
/// write for files.
/// </para>
/// </remarks>
public sealed class FileSystemFileCopier : IFileCopier
{
    public void Copy(string source, string destination, Action<long, long>? progress, CancellationToken ct)
    {
        var cancel = 0;
        var reporter = new Reporter(progress, ct);
        var routine = reporter.Routine;

        var ok = CopyNative.CopyFileExW(
            Extended(source), Extended(destination), routine, IntPtr.Zero,
            ref cancel, CopyNative.CopyFileFailIfExists);

        GC.KeepAlive(routine);
        Finish(ok, destination, reporter, ct);
    }

    public void Move(string source, string destination, Action<long, long>? progress, CancellationToken ct)
    {
        var reporter = new Reporter(progress, ct);
        var routine = reporter.Routine;

        var ok = CopyNative.MoveFileWithProgressW(
            Extended(source), Extended(destination), routine, IntPtr.Zero,
            CopyNative.MoveFileCopyAllowed | CopyNative.MoveFileWriteThrough);

        GC.KeepAlive(routine);
        Finish(ok, destination, reporter, ct);
    }

    private static void Finish(bool ok, string destination, Reporter reporter, CancellationToken ct)
    {
        if (ok)
        {
            reporter.ReportCompletion();
            return;
        }

        var error = Marshal.GetLastWin32Error();
        if (error == CopyNative.ErrorRequestAborted || ct.IsCancellationRequested)
        {
            // Both calls remove the partial destination themselves; make sure of it either way.
            TryDelete(destination);
            throw new OperationCanceledException(ct);
        }

        throw Translate(error);
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Best effort: the source is intact, which is what matters.
        }
    }

    /// <summary>
    /// Maps a Win32 error onto the exception the equivalent <see cref="File"/> call would have
    /// thrown, so <c>TransferExecutor.IsTransferFailure</c> keeps classifying failures as it did.
    /// </summary>
    private static Exception Translate(int error)
    {
        var message = new Win32Exception(error).Message;
        var hresult = unchecked((int)(0x80070000 | (uint)error));

        return error switch
        {
            2 => new FileNotFoundException(message),
            3 => new DirectoryNotFoundException(message),
            CopyNative.ErrorAccessDenied => new UnauthorizedAccessException(message),
            _ => new IOException(message, hresult),
        };
    }

    /// <summary>
    /// Prefixes a long path with <c>\\?\</c>. The .NET file APIs do this for us; the raw Win32
    /// calls do not, so without it a copy into a deep tree fails at MAX_PATH with a message about
    /// the filename being too long. The prefix turns off path normalization, which is safe here
    /// because every path reaching this class is already fully qualified.
    /// </summary>
    /// <remarks>
    /// The prefix is two backslashes, a question mark and a backslash. It was written with one
    /// backslash for a while, which made every long copy fail with "filename syntax is incorrect"
    /// and no short one — <c>TheRealCopier_CopiesIntoAPathLongerThanMaxPath</c> pins it.
    /// </remarks>
    private static string Extended(string path)
    {
        if (path.Length < 260 || path.StartsWith(@"\\?\", StringComparison.Ordinal))
            return path;
        if (path.StartsWith(@"\\", StringComparison.Ordinal))
            return @"\\?\UNC\" + path[2..];
        return Path.IsPathFullyQualified(path) ? @"\\?\" + path : path;
    }

    /// <summary>
    /// Bridges the native callback to the <see cref="Action{T1,T2}"/> the executor passes, and is
    /// where cancellation is actually noticed — the routine is called per chunk, so returning
    /// <see cref="CopyNative.ProgressCancel"/> is what stops a copy part-way through a file.
    /// </summary>
    private sealed class Reporter
    {
        private readonly Action<long, long>? _progress;
        private readonly CancellationToken _ct;
        private long _total;
        private long _transferred;

        internal Reporter(Action<long, long>? progress, CancellationToken ct)
        {
            _progress = progress;
            _ct = ct;
            Routine = OnProgress;
        }

        internal CopyNative.ProgressRoutine Routine { get; }

        /// <summary>A small file can finish having reported nothing, or having reported a figure
        /// short of the total. Either way the caller is owed the full count exactly once.</summary>
        internal void ReportCompletion()
        {
            if (_total > _transferred) _progress?.Invoke(_total, _total);
        }

        private uint OnProgress(
            long totalBytes, long transferred, long streamSize, long streamTransferred,
            uint streamNumber, uint callbackReason, IntPtr source, IntPtr destination, IntPtr data)
        {
            if (_ct.IsCancellationRequested) return CopyNative.ProgressCancel;

            _total = totalBytes;
            _transferred = transferred;
            _progress?.Invoke(transferred, totalBytes);
            return CopyNative.ProgressContinue;
        }
    }
}
