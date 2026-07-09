using System.Buffers.Binary;
using System.Runtime.InteropServices;
using BertBrowser.Core.Data;
using BertBrowser.Core.Interop;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// Indexes one NTFS volume by reading its Master File Table, and keeps it live by tailing
/// the USN change journal. The initial pass (<see cref="BuildInitialIndex"/>) enumerates
/// every record with FSCTL_ENUM_USN_DATA — orders of magnitude faster than walking the
/// tree — reconstructs paths from the flat FRN→parent references, and bulk-upserts them
/// into the shared <c>fs_entry</c> index. <see cref="Tail"/> then applies create / delete /
/// rename records as they happen.
///
/// FSCTL_ENUM_USN_DATA does not return sizes or timestamps, so rows are written with size 0
/// and <see cref="DateTime.MinValue"/>; the UI hydrates those lazily for displayed results.
/// A resident FRN→path map of directories only (a small fraction of all entries) is retained
/// after the initial pass so the tail can resolve change-record paths without a rescan.
/// </summary>
internal sealed class MftVolumeIndexer : IDisposable
{
    private const int EnumBufferSize = 1 << 20;   // 1 MB
    private const int TailBufferSize = 64 * 1024;
    private const int UpsertChunk = 20_000;
    private static readonly TimeSpan TailPollInterval = TimeSpan.FromMilliseconds(1000);

    private readonly FsIndexRepository _repository;
    private readonly string _driveRoot; // "C:\"
    private readonly string _rootKey;   // canonical "C:\"

    private SafeFileHandle? _handle;
    private ulong _journalId;
    private long _nextUsn;
    private long _maxUsn;
    private int _lastError;

    /// <summary>Directory FRN → (display path, effective-hidden). Built during the initial
    /// enum and maintained by the tail; the source of truth for resolving change paths.</summary>
    private readonly Dictionary<ulong, (string Path, bool Hidden)> _dirs = new();

    /// <summary>Old paths captured from RENAME_OLD_NAME records, paired with the matching
    /// RENAME_NEW_NAME/CLOSE record by file reference number.</summary>
    private readonly Dictionary<ulong, string> _pendingRenames = new();

    public string RootKey => _rootKey;
    public string DriveRoot => _driveRoot;

    public MftVolumeIndexer(FsIndexRepository repository, string driveLetter)
    {
        _repository = repository;
        _driveRoot = driveLetter + @":\";
        _rootKey = _driveRoot.ToUpperInvariant();
    }

    /// <summary>Opens the raw volume and activates its USN journal. Returns false (rather
    /// than throwing) if the volume can't be opened or has no usable journal — the caller
    /// simply leaves that drive to the crawl fallback.</summary>
    public bool Open()
    {
        var handle = NtfsNative.CreateFileW(
            $@"\\.\{_driveRoot[..2]}", // "\\.\C:"
            NtfsNative.GenericRead | NtfsNative.GenericWrite,
            NtfsNative.FileShareRead | NtfsNative.FileShareWrite,
            IntPtr.Zero, NtfsNative.OpenExisting, 0, IntPtr.Zero);

        if (handle.IsInvalid)
        {
            handle.Dispose();
            // Retry read-only: creating a journal needs write, but querying an existing one
            // (the common case) does not, and some volumes deny write handles.
            handle = NtfsNative.CreateFileW(
                $@"\\.\{_driveRoot[..2]}", NtfsNative.GenericRead,
                NtfsNative.FileShareRead | NtfsNative.FileShareWrite,
                IntPtr.Zero, NtfsNative.OpenExisting, 0, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                handle.Dispose();
                return false;
            }
        }

        _handle = handle;
        return QueryJournal();
    }

    private bool QueryJournal()
    {
        var buffer = new byte[Marshal.SizeOf<NtfsNative.UsnJournalDataV0>()];
        if (!Ioctl(NtfsNative.FsctlQueryUsnJournal, IntPtr.Zero, 0, buffer, out _))
        {
            if (_lastError is NtfsNative.ErrorJournalNotActive or NtfsNative.ErrorInvalidFunction && CreateJournal())
                return QueryJournal();
            return false;
        }

        var data = MemoryMarshal.Read<NtfsNative.UsnJournalDataV0>(buffer);
        _journalId = data.UsnJournalID;
        _nextUsn = data.NextUsn;
        _maxUsn = data.MaxUsn;
        return true;
    }

    private bool CreateJournal()
    {
        var input = new NtfsNative.CreateUsnJournalData
        {
            MaximumSize = 32 * 1024 * 1024,
            AllocationDelta = 4 * 1024 * 1024,
        };
        return IoctlStruct(NtfsNative.FsctlCreateUsnJournal, input, Array.Empty<byte>(), out _);
    }

    /// <summary>
    /// One full MFT enumeration → path reconstruction → bulk upsert. On completion the
    /// volume root is registered <c>complete</c> so searches route to the index, and the
    /// resident directory map (<see cref="_dirs"/>) is ready for the tail.
    /// </summary>
    public void BuildInitialIndex(CancellationToken ct)
    {
        if (_handle is null) return;

        var crawledUtc = DateTime.UtcNow;
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        var map = EnumerateMft(ct);
        if (ct.IsCancellationRequested) return;

        _dirs.Clear();
        var rows = new List<FsEntryRow>(UpsertChunk);
        foreach (var (frn, node) in map)
        {
            ct.ThrowIfCancellationRequested();
            if (frn == NtfsNative.RootFileReferenceNumber || node.Name is "." or "..")
                continue;
            if (!MftPathBuilder.TryResolve(map, frn, _driveRoot, _dirs, out var display, out var hidden))
                continue;

            // display is already a well-formed absolute path with single separators, so the
            // canonical key is just its invariant-uppercase form — no need for GetFullPath.
            rows.Add(new FsEntryRow(display.ToUpperInvariant(), node.Name, node.IsDirectory, 0, DateTime.MinValue, hidden));
            if (rows.Count >= UpsertChunk)
            {
                _repository.UpsertEntries(rows, crawlGen);
                rows.Clear();
            }
        }
        _repository.UpsertEntries(rows, crawlGen);

        _repository.SweepVanished(_rootKey, crawlGen);
        _repository.UpsertRoot(_rootKey, _driveRoot, crawledUtc, complete: true);
    }

    private Dictionary<ulong, MftNode> EnumerateMft(CancellationToken ct)
    {
        var map = new Dictionary<ulong, MftNode>();
        var buffer = new byte[EnumBufferSize];
        var input = new NtfsNative.MftEnumDataV0
        {
            StartFileReferenceNumber = 0,
            LowUsn = 0,
            HighUsn = _maxUsn,
        };

        while (!ct.IsCancellationRequested)
        {
            if (!IoctlStruct(NtfsNative.FsctlEnumUsnData, input, buffer, out var bytes))
            {
                // ERROR_HANDLE_EOF is the normal end of the table.
                break;
            }
            if (bytes <= sizeof(ulong))
                break;

            foreach (var rec in UsnRecordParser.Parse(buffer, sizeof(ulong), bytes))
                map[rec.FileReferenceNumber] = new MftNode(rec.Name, rec.ParentFileReferenceNumber, rec.IsDirectory, rec.IsHidden);

            input.StartFileReferenceNumber = BinaryPrimitives.ReadUInt64LittleEndian(buffer);
        }
        return map;
    }

    /// <summary>Reads the USN journal in a loop, applying batches of changes until cancelled.
    /// On journal loss (overflow/deleted) the root is flagged stale and the loop exits so the
    /// orchestrator can re-enumerate.</summary>
    public void Tail(CancellationToken ct)
    {
        if (_handle is null) return;
        var buffer = new byte[TailBufferSize];

        while (!ct.IsCancellationRequested)
        {
            var input = new NtfsNative.ReadUsnJournalDataV0
            {
                StartUsn = _nextUsn,
                ReasonMask = 0xFFFFFFFF,
                ReturnOnlyOnClose = 0,
                Timeout = 0,
                BytesToWaitFor = 0,
                UsnJournalID = _journalId,
            };

            if (!IoctlStruct(NtfsNative.FsctlReadUsnJournal, input, buffer, out var bytes))
            {
                _repository.MarkRootStale(_rootKey);
                return;
            }

            var next = BinaryPrimitives.ReadInt64LittleEndian(buffer);
            if (bytes > sizeof(long) && next != _nextUsn)
            {
                Apply(UsnRecordParser.Parse(buffer, sizeof(long), bytes));
                _nextUsn = next;
            }
            else
            {
                _nextUsn = next;
                if (ct.WaitHandle.WaitOne(TailPollInterval))
                    return;
            }
        }
    }

    /// <summary>Applies one drained batch of journal records. Records arrive in USN order,
    /// so a directory's create always precedes its children's. Non-CLOSE records are ignored
    /// except RENAME_OLD_NAME, which is captured to pair with the matching new-name record.</summary>
    private void Apply(IEnumerable<UsnRecord> records)
    {
        foreach (var rec in records)
        {
            if ((rec.Reason & NtfsNative.UsnReasonRenameOldName) != 0)
            {
                if (TryResolvePath(rec, out var oldPath, out _))
                    _pendingRenames[rec.FileReferenceNumber] = oldPath;
                continue;
            }

            if ((rec.Reason & NtfsNative.UsnReasonClose) == 0)
                continue; // wait for the coalescing CLOSE record

            if ((rec.Reason & NtfsNative.UsnReasonFileDelete) != 0)
                ApplyDelete(rec);
            else if ((rec.Reason & NtfsNative.UsnReasonRenameNewName) != 0)
                ApplyRename(rec);
            else
                ApplyUpsert(rec);
        }
    }

    private void ApplyDelete(UsnRecord rec)
    {
        if (!TryResolvePath(rec, out _, out var key))
            return;
        _repository.DeleteSubtree(key);
        if (rec.IsDirectory)
            RemoveDirSubtree(rec.FileReferenceNumber, key);
    }

    private void ApplyRename(UsnRecord rec)
    {
        if (!TryResolvePath(rec, out var newDisplay, out var newKey))
            return;

        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (_pendingRenames.Remove(rec.FileReferenceNumber, out var oldPath))
        {
            var oldKey = oldPath.ToUpperInvariant();
            _repository.Rename(oldKey, newKey, rec.Name, crawlGen);
            if (rec.IsDirectory)
                RewriteDirSubtree(oldKey, newKey, newDisplay, rec);
        }
        else
        {
            // No captured old name (moved in from elsewhere / lost) — treat as a fresh entry.
            ApplyUpsert(rec);
        }
    }

    private void ApplyUpsert(UsnRecord rec)
    {
        if (!TryResolvePath(rec, out var display, out var key))
            return;

        var hidden = ParentHidden(rec.ParentFileReferenceNumber) || rec.IsHidden;
        var (size, modified) = StatBestEffort(display, rec.IsDirectory);
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        _repository.UpsertEntries(new[] { new FsEntryRow(key, rec.Name, rec.IsDirectory, size, modified, hidden) }, crawlGen);

        if (rec.IsDirectory)
            _dirs[rec.FileReferenceNumber] = (display, hidden);
    }

    /// <summary>Resolves the display path (and canonical key) of a change record from its
    /// parent directory in the resident map. Fails when the parent is unknown (skips the
    /// record — best-effort, matching the crawl fallback's tolerance).</summary>
    private bool TryResolvePath(UsnRecord rec, out string display, out string key)
    {
        display = "";
        key = "";
        if (rec.ParentFileReferenceNumber == NtfsNative.RootFileReferenceNumber)
        {
            display = _driveRoot + rec.Name;
        }
        else if (_dirs.TryGetValue(rec.ParentFileReferenceNumber, out var parent))
        {
            display = parent.Path.EndsWith('\\') ? parent.Path + rec.Name : parent.Path + '\\' + rec.Name;
        }
        else
        {
            return false;
        }
        key = display.ToUpperInvariant();
        return true;
    }

    private bool ParentHidden(ulong parentFrn) =>
        parentFrn != NtfsNative.RootFileReferenceNumber
        && _dirs.TryGetValue(parentFrn, out var parent) && parent.Hidden;

    private static (long Size, DateTime Modified) StatBestEffort(string path, bool isDirectory)
    {
        try
        {
            if (isDirectory)
                return (0, new DirectoryInfo(path).LastWriteTimeUtc);
            var info = new FileInfo(path);
            return (info.Length, info.LastWriteTimeUtc);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return (0, DateTime.MinValue);
        }
    }

    /// <summary>Drops a deleted directory and all descendant directories from the resident map.</summary>
    private void RemoveDirSubtree(ulong frn, string key)
    {
        _dirs.Remove(frn);
        var (lo, hi) = PathKey.PrefixBounds(key);
        foreach (var stale in _dirs.Where(kv => string.CompareOrdinal(kv.Value.Path.ToUpperInvariant(), lo) >= 0
                                              && string.CompareOrdinal(kv.Value.Path.ToUpperInvariant(), hi) < 0)
                                   .Select(kv => kv.Key).ToList())
            _dirs.Remove(stale);
    }

    /// <summary>Rewrites the moved directory and every descendant directory's cached path so
    /// later child changes resolve correctly. Dir renames are rare relative to file churn, so
    /// the linear scan over the (small) directory map is acceptable.</summary>
    private void RewriteDirSubtree(string oldKey, string newKey, string newDisplay, UsnRecord rec)
    {
        var hidden = ParentHidden(rec.ParentFileReferenceNumber) || rec.IsHidden;
        _dirs[rec.FileReferenceNumber] = (newDisplay, hidden);

        var (lo, hi) = PathKey.PrefixBounds(oldKey);
        foreach (var (frn, value) in _dirs.ToList())
        {
            var upper = value.Path.ToUpperInvariant();
            if (string.CompareOrdinal(upper, lo) >= 0 && string.CompareOrdinal(upper, hi) < 0)
                _dirs[frn] = (newDisplay + value.Path[oldKey.Length..], value.Hidden);
        }
    }

    // --- DeviceIoControl helpers ---

    private bool Ioctl(uint code, IntPtr inPtr, int inSize, byte[] output, out int bytesReturned)
    {
        var pinned = GCHandle.Alloc(output, GCHandleType.Pinned);
        try
        {
            var outPtr = output.Length == 0 ? IntPtr.Zero : pinned.AddrOfPinnedObject();
            var ok = NtfsNative.DeviceIoControl(_handle!, code, inPtr, inSize, outPtr, output.Length, out bytesReturned, IntPtr.Zero);
            _lastError = ok ? 0 : Marshal.GetLastWin32Error();
            return ok;
        }
        finally
        {
            pinned.Free();
        }
    }

    private bool IoctlStruct<T>(uint code, in T input, byte[] output, out int bytesReturned) where T : struct
    {
        var size = Marshal.SizeOf<T>();
        var inPtr = Marshal.AllocHGlobal(size);
        try
        {
            Marshal.StructureToPtr(input, inPtr, false);
            return Ioctl(code, inPtr, size, output, out bytesReturned);
        }
        finally
        {
            Marshal.DestroyStructure<T>(inPtr);
            Marshal.FreeHGlobal(inPtr);
        }
    }

    public void Dispose() => _handle?.Dispose();
}
