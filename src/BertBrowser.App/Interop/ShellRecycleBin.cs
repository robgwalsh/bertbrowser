using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using BertBrowser.Core.Services.Delete;

namespace BertBrowser.App.Interop;

/// <summary>
/// The Windows Recycle Bin, over <c>IFileOperation</c>.
/// </summary>
/// <remarks>
/// <para>
/// <c>IFileOperation</c> rather than the older <c>SHFileOperation</c> for one decisive reason: its
/// progress sink's <c>PostDeleteItem</c> hands back <c>psiNewlyCreated</c>, which is the item as it
/// now exists inside the bin (<c>$Recycle.Bin\&lt;SID&gt;\$RXXXXXX.ext</c>) — or null when the shell
/// erased it rather than holding it. Capturing that path is what lets undo restore <em>this</em>
/// item rather than searching the bin for something whose original path looks similar, which is the
/// difference between correct and nearly-correct when the same path has been deleted twice.
/// </para>
/// <para>
/// <b>The flags are the dangerous part.</b> <c>FOF_ALLOWUNDO</c> together with
/// <c>FOF_NOCONFIRMATION</c> means "if it cannot be recycled, erase it without asking", which on a
/// full bin or a network share destroys data silently — the opposite of everything this app's
/// delete design stands for. Two things stand against that: the planner routes items on volumes
/// with no bin to the staging folder before this class ever sees them, and
/// <c>FOF_WANTNUKEWARNING</c> stays set so the one case pre-flight cannot predict — an item over
/// the bin's quota — still asks. That is the only OS-drawn dialog this app allows, and it is
/// deliberate: every other confirmation, progress bar and error box is suppressed so the themed
/// <c>DeleteDialog</c> stays the confirmation the user actually answers.
/// </para>
/// <para>
/// The COM interfaces below are hand-declared, so <b>every method ahead of the one we call must be
/// present, in vtable order</b>. A missing slot lands the call somewhere else entirely, which is an
/// access violation rather than a catchable exception.
/// </para>
/// <para>
/// Everything runs on a dedicated STA thread with a deadline, like
/// <see cref="ShellLauncher"/> and <see cref="PortableDevices"/>: <c>IFileOperation</c> is
/// apartment-threaded and may put up UI.
/// </para>
/// </remarks>
public sealed class ShellRecycleBin : IRecycleBin, IRecycleProbe
{
    /// <summary>Long enough for a big batch of renames — recycling does not copy — and short enough
    /// that a wedged shell does not hang the app forever.</summary>
    private static readonly TimeSpan OperationTimeout = TimeSpan.FromMinutes(10);

    /// <summary>A restore is a shell verb that reports nothing, so success is observed rather than
    /// returned. See <see cref="Restore"/>.</summary>
    private static readonly TimeSpan RestoreTimeout = TimeSpan.FromSeconds(5);

    /// <summary>Per-volume answers, cached for the life of this object: a plan asks once per item
    /// and the answer does not change between the plan and its execution in any way that matters.</summary>
    private readonly Dictionary<string, bool> _canRecycle = new(StringComparer.OrdinalIgnoreCase);

    private readonly Lock _cacheLock = new();

    // --- IRecycleProbe ---

    /// <inheritdoc/>
    public bool CanRecycle(string path)
    {
        string root;
        try
        {
            root = Path.GetPathRoot(Path.GetFullPath(path)) ?? "";
        }
        catch (Exception ex) when (IsShellFailure(ex))
        {
            return false;
        }
        if (root.Length == 0) return false;

        lock (_cacheLock)
        {
            if (_canRecycle.TryGetValue(root, out var cached)) return cached;
            var answer = QueryCanRecycle(root);
            _canRecycle[root] = answer;
            return answer;
        }
    }

    /// <summary>
    /// A network share has no Recycle Bin at all, and a volume that is not ready cannot be asked.
    /// Beyond that, <c>SHQueryRecycleBin</c> succeeding is the shell's own answer to "is there a bin
    /// here". The per-volume "don't move files to the Recycle Bin" setting is deliberately not
    /// probed — finding it means resolving a volume GUID — because <c>FOF_WANTNUKEWARNING</c>
    /// catches that case honestly at the point of deletion instead.
    /// </summary>
    private static bool QueryCanRecycle(string root)
    {
        try
        {
            var drive = new DriveInfo(root);
            if (!drive.IsReady) return false;
            if (drive.DriveType is DriveType.Network or DriveType.NoRootDirectory) return false;
        }
        catch (Exception ex) when (IsShellFailure(ex))
        {
            return false;
        }

        try
        {
            var info = new ShQueryRbInfo { CbSize = Marshal.SizeOf<ShQueryRbInfo>() };
            return SHQueryRecycleBin(root, ref info) == 0;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            return false;
        }
    }

    // --- IRecycleBin ---

    /// <inheritdoc/>
    public RecycleResult Recycle(
        IReadOnlyList<PlannedDelete> items,
        CancellationToken ct = default,
        IProgress<DeleteProgress>? progress = null)
    {
        if (items.Count == 0) return new RecycleResult([], []);

        RecycleResult? result = null;
        Exception? failure = null;

        var thread = new Thread(() =>
        {
            try
            {
                result = RecycleCore(items, ct, progress);
            }
            catch (Exception ex)
            {
                // Carried back rather than thrown here: an exception on this thread would be
                // unobserved and would take the process down.
                failure = ex;
            }
        })
        {
            IsBackground = true,
            Name = "Recycle Bin",
        };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();

        if (!thread.Join(OperationTimeout))
            return AllFailed(items, "the Recycle Bin did not respond.");

        if (result is { } done) return done;
        return AllFailed(items, failure?.Message ?? "the Recycle Bin could not be reached.");
    }

    private static RecycleResult AllFailed(IReadOnlyList<PlannedDelete> items, string reason) =>
        new([], items.Select(i => new FailedDelete(i.SourcePath, $"{i.Name}: {reason}")).ToList());

    private static RecycleResult RecycleCore(
        IReadOnlyList<PlannedDelete> items, CancellationToken ct, IProgress<DeleteProgress>? progress)
    {
        var type = Type.GetTypeFromCLSID(ClsidFileOperation)
            ?? throw new InvalidOperationException("The shell's file-operation object is unavailable.");
        if (Activator.CreateInstance(type) is not IFileOperation operation)
            throw new InvalidOperationException("The shell's file-operation object could not be created.");

        var sink = new DeleteSink(items, progress);
        var cookie = 0u;
        try
        {
            operation.SetOperationFlags(OperationFlags);
            operation.Advise(sink, out cookie);

            // Everything is added first and performed once: that is what gives a single sink, and
            // it is also far faster than one operation per item.
            //
            // Each add is guarded separately, and that guard is load-bearing. Resolving a path that
            // has since gone throws here — before PerformOperations has run — so a single missing
            // item would otherwise abort the batch and report every one of its siblings as failed,
            // having deleted none of them. One item's failure must never cost the others.
            var added = 0;
            foreach (var item in items)
            {
                ct.ThrowIfCancellationRequested();
                try
                {
                    var shellItem = ItemFor(item.SourcePath);
                    try
                    {
                        operation.DeleteItem(shellItem, null);
                        added++;
                    }
                    finally
                    {
                        Marshal.ReleaseComObject(shellItem);
                    }
                }
                catch (Exception ex) when (IsShellFailure(ex))
                {
                    sink.NoteItemFailure(item, ex.Message);
                }
            }

            // Performing an empty operation is legal but pointless, and skipping it keeps the shell
            // from putting anything on screen when every item was already gone.
            if (added > 0) operation.PerformOperations();
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex) when (IsShellFailure(ex))
        {
            // Whatever the sink already recorded still stands; the rest are reported as failures.
            sink.NoteBatchFailure(ex.Message);
        }
        finally
        {
            try
            {
                if (cookie != 0) operation.Unadvise(cookie);
            }
            catch (Exception ex) when (IsShellFailure(ex))
            {
            }
            Marshal.FinalReleaseComObject(operation);
        }

        return sink.Result();
    }

    /// <inheritdoc/>
    /// <remarks>
    /// Restoring goes through the shell's own <c>undelete</c> verb, so the bin's bookkeeping — the
    /// paired <c>$I</c> metadata file — is cleaned up by the code that owns it. The canonical verb
    /// name is used rather than the menu text, which is localised.
    /// <para>
    /// <c>InvokeVerb</c> returns nothing at all, so success is established by watching for the
    /// original path to come back. A timeout is reported as a failure naming the <c>$R</c> path, so
    /// the data is findable rather than quietly lost.
    /// </para>
    /// <para>
    /// One genuine difference from the staged undo: the shell recreates a missing parent folder
    /// chain, where restoring from the holding folder refuses.
    /// </para>
    /// </remarks>
    public bool Restore(DeletedItem item)
    {
        if (item.RecycledPath is not { Length: > 0 } held) return false;

        var restored = false;
        var thread = new Thread(() =>
        {
            try
            {
                restored = RestoreCore(held, item.SourcePath);
            }
            catch
            {
                // Reported by the caller as "the bin no longer holds it", which is what a failure
                // here means in every case we can distinguish.
            }
        })
        {
            IsBackground = true,
            Name = "Recycle Bin restore",
        };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join(RestoreTimeout + TimeSpan.FromSeconds(5));
        return restored;
    }

    private static bool RestoreCore(string heldPath, string sourcePath)
    {
        var shellType = Type.GetTypeFromProgID("Shell.Application");
        if (shellType is null) return false;

        dynamic? shell = Activator.CreateInstance(shellType);
        if (shell is null) return false;

        try
        {
            dynamic bin = shell.NameSpace(SsfBitBucket);
            if (bin is null) return false;

            dynamic entries = bin.Items();
            foreach (dynamic entry in entries)
            {
                string path = entry.Path;
                if (!string.Equals(path, heldPath, StringComparison.OrdinalIgnoreCase)) continue;

                if (!InvokeUndelete(entry)) return false;

                // The verb reports nothing, so the only honest answer is to watch for the file.
                var deadline = DateTime.UtcNow + RestoreTimeout;
                while (DateTime.UtcNow < deadline)
                {
                    if (File.Exists(sourcePath) || Directory.Exists(sourcePath)) return true;
                    Thread.Sleep(50);
                }
                return false;
            }
            return false; // emptied, swept by Storage Sense, or already restored by hand
        }
        finally
        {
            Marshal.FinalReleaseComObject(shell);
        }
    }

    /// <summary>The canonical verb name is stable across languages; the displayed one ("Restore",
    /// "&amp;Restore", and every translation) is not, so it is only a fallback.</summary>
    private static bool InvokeUndelete(dynamic entry)
    {
        try
        {
            entry.InvokeVerb("undelete");
            return true;
        }
        catch (Exception ex) when (IsShellFailure(ex) || ex is Microsoft.CSharp.RuntimeBinder.RuntimeBinderException)
        {
        }

        try
        {
            foreach (dynamic verb in entry.Verbs())
            {
                string name = verb.Name;
                if (name.Replace("&", "").Equals("Restore", StringComparison.OrdinalIgnoreCase))
                {
                    verb.DoIt();
                    return true;
                }
            }
        }
        catch (Exception ex) when (IsShellFailure(ex) || ex is Microsoft.CSharp.RuntimeBinder.RuntimeBinderException)
        {
        }
        return false;
    }

    // --- The progress sink ---

    /// <summary>
    /// Records what happened to each item. <c>PostDeleteItem</c> is the only reason this exists:
    /// <c>psiNewlyCreated</c> is the item's new home in the bin, and null means the shell erased it
    /// instead — not a failure, but nothing to undo either.
    /// </summary>
    private sealed class DeleteSink(
        IReadOnlyList<PlannedDelete> items, IProgress<DeleteProgress>? progress)
        : IFileOperationProgressSink
    {
        private readonly Dictionary<string, PlannedDelete> _bySource =
            items.ToDictionary(i => i.SourcePath, StringComparer.OrdinalIgnoreCase);

        private readonly List<RecycledItem> _recycled = [];
        private readonly List<FailedDelete> _failed = [];
        private readonly HashSet<string> _seen = new(StringComparer.OrdinalIgnoreCase);
        private int _done;
        private string? _batchFailure;

        public void NoteBatchFailure(string message) => _batchFailure = message;

        /// <summary>Records one item the shell would not even accept — most often a path that has
        /// gone since the plan was made. Marked as seen so <see cref="Result"/> does not report it
        /// a second time under the generic message.</summary>
        public void NoteItemFailure(PlannedDelete item, string message)
        {
            _seen.Add(item.SourcePath);
            _failed.Add(new FailedDelete(item.SourcePath, $"{item.Name}: {message}"));
        }

        public RecycleResult Result()
        {
            // Anything the shell never reported on did not happen. Saying so beats leaving the
            // caller to believe an item was deleted when it is still on disk.
            foreach (var item in items)
            {
                if (_seen.Contains(item.SourcePath)) continue;
                _failed.Add(new FailedDelete(item.SourcePath,
                    $"{item.Name}: {_batchFailure ?? "the Recycle Bin did not take it."}"));
            }
            return new RecycleResult(_recycled, _failed);
        }

        public int PreDeleteItem(uint dwFlags, IShellItem psiItem)
        {
            if (PathOf(psiItem) is { } path && _bySource.TryGetValue(path, out var planned))
                progress?.Report(new DeleteProgress(_done, items.Count, planned.Name));
            return 0;
        }

        public int PostDeleteItem(
            uint dwFlags, IShellItem psiItem, int hrDelete, IShellItem? psiNewlyCreated)
        {
            _done++;

            if (PathOf(psiItem) is not { } source || !_bySource.TryGetValue(source, out var planned))
                return 0; // a child of a folder we deleted; only the roots we asked for are recorded

            _seen.Add(source);

            if (hrDelete < 0)
            {
                _failed.Add(new FailedDelete(
                    planned.SourcePath, $"{planned.Name}: {Marshal.GetExceptionForHR(hrDelete)?.Message ?? "could not be deleted."}"));
                return 0;
            }

            // Null means it was erased rather than held — over the bin's quota, most often. The
            // item is gone either way; what changes is whether there is anything to undo.
            _recycled.Add(new RecycledItem(
                planned.SourcePath, planned.IsDirectory, psiNewlyCreated is null ? null : PathOf(psiNewlyCreated)));
            return 0;
        }

        // --- Everything below is vtable padding: unused, but every slot must be present and in
        //     order, or the calls above land on the wrong one. ---

        public int StartOperations() => 0;

        public int FinishOperations(int hrResult) => 0;

        public int PreRenameItem(uint dwFlags, IShellItem psiItem, string? pszNewName) => 0;

        public int PostRenameItem(
            uint dwFlags, IShellItem psiItem, string? pszNewName, int hrRename, IShellItem? psiNewlyCreated) => 0;

        public int PreMoveItem(
            uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder, string? pszNewName) => 0;

        public int PostMoveItem(
            uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder, string? pszNewName,
            int hrMove, IShellItem? psiNewlyCreated) => 0;

        public int PreCopyItem(
            uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder, string? pszNewName) => 0;

        public int PostCopyItem(
            uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder, string? pszNewName,
            int hrCopy, IShellItem? psiNewlyCreated) => 0;

        public int PreNewItem(uint dwFlags, IShellItem psiDestinationFolder, string? pszNewName) => 0;

        public int PostNewItem(
            uint dwFlags, IShellItem psiDestinationFolder, string? pszNewName, string? pszTemplateName,
            uint dwFileAttributes, int hrNew, IShellItem? psiNewItem) => 0;

        public int UpdateProgress(uint iWorkTotal, uint iWorkSoFar) => 0;

        public int ResetTimer() => 0;

        public int PauseTimer() => 0;

        public int ResumeTimer() => 0;
    }

    // --- Shell plumbing ---

    private static IShellItem ItemFor(string path)
    {
        var iid = IidShellItem;
        SHCreateItemFromParsingName(path, IntPtr.Zero, ref iid, out var item);
        return item;
    }

    private static string? PathOf(IShellItem item)
    {
        var buffer = IntPtr.Zero;
        try
        {
            item.GetDisplayName(SigdnFileSysPath, out buffer);
            return buffer == IntPtr.Zero ? null : Marshal.PtrToStringUni(buffer);
        }
        catch (Exception ex) when (IsShellFailure(ex))
        {
            return null; // not a filesystem item; nothing we can match against
        }
        finally
        {
            if (buffer != IntPtr.Zero) Marshal.FreeCoTaskMem(buffer);
        }
    }

    /// <summary>Errors that mean the shell refused, not that the program is broken.</summary>
    private static bool IsShellFailure(Exception ex) =>
        ex is COMException or IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or InvalidOperationException or System.Security.SecurityException;

    // --- Constants ---

    private static readonly Guid ClsidFileOperation = new("3ad05575-8857-4850-9277-11b85bdb8e09");
    private static readonly Guid IidShellItem = new("43826d1e-e718-42ee-bc55-a1e261c37bfe");

    /// <summary>ssfBITBUCKET — the Recycle Bin shell folder.</summary>
    private const int SsfBitBucket = 10;

    /// <summary>SIGDN_FILESYSPATH.</summary>
    private const uint SigdnFileSysPath = 0x80058000;

    private const uint FofSilent = 0x0004;
    private const uint FofNoConfirmation = 0x0010;
    private const uint FofAllowUndo = 0x0040;
    private const uint FofNoConfirmMkDir = 0x0200;
    private const uint FofNoErrorUi = 0x0400;
    private const uint FofWantNukeWarning = 0x4000;
    private const uint FofxNoMinimizeBox = 0x01000000;
    private const uint FofxRecycleOnDelete = 0x00080000;
    private const uint FofxAddUndoRecord = 0x20000000;

    /// <summary>
    /// Three flags say "recycle, do not erase" — <c>FOF_ALLOWUNDO</c> is the long-standing one,
    /// <c>FOFX_RECYCLEONDELETE</c> and <c>FOFX_ADDUNDORECORD</c> are its modern spellings — because
    /// the cost of one of them being wrong is the user's files. The suppression flags silence the
    /// shell's own progress, confirmation and error UI, since this app draws its own; the one
    /// deliberate exception is <c>FOF_WANTNUKEWARNING</c>, which overrides
    /// <c>FOF_NOCONFIRMATION</c> for exactly the "this cannot be recycled — delete permanently?"
    /// case that pre-flight cannot predict.
    /// <para>
    /// <c>FOFX_EARLYFAILURE</c> is deliberately absent: it would abandon the rest of the batch on
    /// the first error, and everything else in this app's delete path holds that one item's failure
    /// must never cost the others.
    /// </para>
    /// </summary>
    private const uint OperationFlags =
        FofAllowUndo | FofxRecycleOnDelete | FofxAddUndoRecord |
        FofSilent | FofNoConfirmation | FofNoErrorUi | FofNoConfirmMkDir | FofxNoMinimizeBox |
        FofWantNukeWarning;

    [StructLayout(LayoutKind.Sequential)]
    private struct ShQueryRbInfo
    {
        public int CbSize;
        public long NumBytes;
        public long NumItems;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern int SHQueryRecycleBin(string? pszRootPath, ref ShQueryRbInfo pSHQueryRBInfo);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    private static extern void SHCreateItemFromParsingName(
        string pszPath, IntPtr pbc, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out IShellItem ppv);
}

/// <summary>Only <c>GetDisplayName</c> is used, but the four slots around it must be declared in
/// order or the call lands elsewhere.</summary>
[ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IShellItem
{
    void BindToHandler(IntPtr pbc, ref Guid bhid, ref Guid riid, out IntPtr ppv);

    void GetParent(out IShellItem ppsi);

    void GetDisplayName(uint sigdnName, out IntPtr ppszName);

    void GetAttributes(uint sfgaoMask, out uint psfgaoAttribs);

    void Compare(IShellItem psi, uint hint, out int piOrder);
}

/// <summary>
/// The shell's file-operation object. Only <c>SetOperationFlags</c>, <c>Advise</c>,
/// <c>Unadvise</c>, <c>DeleteItem</c> and <c>PerformOperations</c> are called, but all twenty
/// methods are declared because the vtable has no gaps to spare.
/// </summary>
[ComImport, Guid("947aab5f-0a5c-4c13-b4d6-4bf7836fc9f8"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IFileOperation
{
    void Advise(IFileOperationProgressSink pfops, out uint pdwCookie);

    void Unadvise(uint dwCookie);

    void SetOperationFlags(uint dwOperationFlags);

    void SetProgressMessage([MarshalAs(UnmanagedType.LPWStr)] string pszMessage);

    void SetProgressDialog(IntPtr popd);

    void SetProperties(IntPtr pproparray);

    void SetOwnerWindow(IntPtr hwndOwner);

    void ApplyPropertiesToItem(IShellItem psiItem);

    void ApplyPropertiesToItems(IntPtr punkItems);

    void RenameItem(
        IShellItem psiItem,
        [MarshalAs(UnmanagedType.LPWStr)] string pszNewName,
        IFileOperationProgressSink? pfopsItem);

    void RenameItems(IntPtr pUnkItems, [MarshalAs(UnmanagedType.LPWStr)] string pszNewName);

    void MoveItem(
        IShellItem psiItem,
        IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName,
        IFileOperationProgressSink? pfopsItem);

    void MoveItems(IntPtr punkItems, IShellItem psiDestinationFolder);

    void CopyItem(
        IShellItem psiItem,
        IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszCopyName,
        IFileOperationProgressSink? pfopsItem);

    void CopyItems(IntPtr punkItems, IShellItem psiDestinationFolder);

    void DeleteItem(IShellItem psiItem, IFileOperationProgressSink? pfopsItem);

    void DeleteItems(IntPtr punkItems);

    void NewItem(
        IShellItem psiDestinationFolder,
        uint dwFileAttributes,
        [MarshalAs(UnmanagedType.LPWStr)] string pszName,
        [MarshalAs(UnmanagedType.LPWStr)] string pszTemplateName,
        IFileOperationProgressSink? pfopsItem);

    void PerformOperations();

    void GetAnyOperationsAborted([MarshalAs(UnmanagedType.Bool)] out bool pfAnyOperationsAborted);
}

/// <summary>
/// Implemented by this app rather than called, so every method must be present, in order, and each
/// must return an HRESULT — hence <see cref="PreserveSigAttribute"/> throughout. Returning anything
/// negative would abort the operation.
/// </summary>
[ComImport, Guid("04b0f1a7-9490-44bc-96e1-4296a31252e2"),
 InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IFileOperationProgressSink
{
    [PreserveSig] int StartOperations();

    [PreserveSig] int FinishOperations(int hrResult);

    [PreserveSig] int PreRenameItem(
        uint dwFlags, IShellItem psiItem, [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName);

    [PreserveSig] int PostRenameItem(
        uint dwFlags, IShellItem psiItem, [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName,
        int hrRename, IShellItem? psiNewlyCreated);

    [PreserveSig] int PreMoveItem(
        uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName);

    [PreserveSig] int PostMoveItem(
        uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName, int hrMove, IShellItem? psiNewlyCreated);

    [PreserveSig] int PreCopyItem(
        uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName);

    [PreserveSig] int PostCopyItem(
        uint dwFlags, IShellItem psiItem, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName, int hrCopy, IShellItem? psiNewlyCreated);

    [PreserveSig] int PreDeleteItem(uint dwFlags, IShellItem psiItem);

    [PreserveSig] int PostDeleteItem(
        uint dwFlags, IShellItem psiItem, int hrDelete, IShellItem? psiNewlyCreated);

    [PreserveSig] int PreNewItem(
        uint dwFlags, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName);

    [PreserveSig] int PostNewItem(
        uint dwFlags, IShellItem psiDestinationFolder,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszNewName,
        [MarshalAs(UnmanagedType.LPWStr)] string? pszTemplateName,
        uint dwFileAttributes, int hrNew, IShellItem? psiNewItem);

    [PreserveSig] int UpdateProgress(uint iWorkTotal, uint iWorkSoFar);

    [PreserveSig] int ResetTimer();

    [PreserveSig] int PauseTimer();

    [PreserveSig] int ResumeTimer();
}
