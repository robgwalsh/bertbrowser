using System.Buffers.Binary;
using System.Runtime.InteropServices;
using BertBrowser.Core.Data;
using BertBrowser.Core.Interop;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// Indexes one NTFS volume and keeps it live via the USN change journal. The initial pass reads
/// the raw <c>$MFT</c> (<see cref="MftReader"/>) to get names, parents, sizes and timestamps in
/// one sweep — populating the <c>fs_entry</c> search index <em>and</em> a full <c>dir_size_cache</c>
/// rollup so every folder shows its size instantly. If the raw read can't parse the volume it
/// falls back to <see cref="EnumerateMft"/> (FSCTL_ENUM_USN_DATA — names only, no sizes; the DFS
/// scanner still handles sizing there). <see cref="Tail"/> then applies create / delete / rename
/// records as they happen.
///
/// Everything is keyed by 48-bit MFT record numbers (root = 5). USN journal references are the
/// same record number in their low 48 bits, so the tail masks them to look up the resident
/// directory map built by the initial pass.
/// </summary>
internal sealed class MftVolumeIndexer : IDisposable
{
    private const int EnumBufferSize = 1 << 20;   // 1 MB
    private const int TailBufferSize = 64 * 1024;
    private const int UpsertChunk = 20_000;
    private static readonly TimeSpan TailPollInterval = TimeSpan.FromMilliseconds(1000);

    private readonly FsIndexRepository _repository;
    private readonly DirSizeRepository _dirSizeRepository;
    private readonly string _driveRoot; // "C:\"
    private readonly string _rootKey;   // canonical "C:\"

    private SafeFileHandle? _handle;
    private ulong _journalId;
    private long _nextUsn;
    private long _maxUsn;
    private int _lastError;

    /// <summary>Directory record number → (display path, effective-hidden). Built during the
    /// initial pass and maintained by the tail; the source of truth for resolving change paths.</summary>
    private readonly Dictionary<ulong, (string Path, bool Hidden)> _dirs = new();

    /// <summary>Old paths captured from RENAME_OLD_NAME records, keyed by record number.</summary>
    private readonly Dictionary<ulong, string> _pendingRenames = new();

    public string RootKey => _rootKey;
    public string DriveRoot => _driveRoot;

    public MftVolumeIndexer(FsIndexRepository repository, DirSizeRepository dirSizeRepository, string driveLetter)
    {
        _repository = repository;
        _dirSizeRepository = dirSizeRepository;
        _driveRoot = driveLetter + @":\";
        _rootKey = _driveRoot.ToUpperInvariant();
    }

    /// <summary>Opens the raw volume and activates its USN journal. Returns false (rather
    /// than throwing) if the volume can't be opened or has no usable journal.</summary>
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
            // (and reading the MFT) does not, and some volumes deny write handles.
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
    /// Builds the index for the volume: raw <c>$MFT</c> first (names + sizes + dates + folder-size
    /// rollup), falling back to the names-only USN enumeration if the raw read fails. On success
    /// the volume root is registered <c>complete</c> and the resident directory map is ready.
    /// </summary>
    public void BuildInitialIndex(CancellationToken ct)
    {
        if (_handle is null) return;

        var crawledUtc = DateTime.UtcNow;
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();

        if (!BuildFromRawMft(ct, crawlGen))
        {
            ct.ThrowIfCancellationRequested();
            BuildFromUsnEnum(ct, crawlGen);
        }
        ct.ThrowIfCancellationRequested();

        _repository.SweepVanished(_rootKey, crawlGen);
        _repository.UpsertRoot(_rootKey, _driveRoot, crawledUtc, complete: true);
    }

    /// <summary>Raw-$MFT build: fs_entry rows with real sizes/dates plus the dir_size_cache
    /// rollup. Returns false (without writing fs_entry) if the volume can't be parsed.</summary>
    private bool BuildFromRawMft(CancellationToken ct, long crawlGen)
    {
        var reader = new MftReader(_handle!);
        var records = new List<MftFileRecord>();

        try
        {
            if (!reader.TryReadAll(ct, records.Add))
                return false;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception)
        {
            return false; // unexpected layout / read error — fall back to the USN enumeration
        }

        var computedUtc = DateTime.UtcNow;

        // Resolve every directory's path first: needed both for fs_entry keys and to stat the
        // handful of heavily-fragmented files that came back with an unknown (-1) size.
        var dirNodes = new Dictionary<ulong, MftNode>();
        foreach (var rec in records)
            if (rec.IsDirectory)
                dirNodes[rec.RecordNumber] = new MftNode(rec.Name, rec.ParentRecordNumber, true, rec.Hidden);

        _dirs.Clear();
        foreach (var recno in dirNodes.Keys)
            MftPathBuilder.TryResolve(dirNodes, recno, _driveRoot, _dirs, out _, out _, NtfsLayout.RootRecordNumber);

        var sizes = new MftDirectorySizeBuilder();
        var rows = new List<FsEntryRow>(UpsertChunk);
        foreach (var raw in records)
        {
            ct.ThrowIfCancellationRequested();
            var rec = raw.Size >= 0 ? raw : raw with { Size = StatFileSize(raw) };
            sizes.Add(rec);
            if (TryResolveRecord(rec, out var key, out var hidden))
            {
                rows.Add(new FsEntryRow(key, rec.Name, rec.IsDirectory, rec.IsDirectory ? 0 : rec.Size, rec.ModifiedUtc, hidden));
                if (rows.Count >= UpsertChunk)
                {
                    _repository.UpsertEntries(rows, crawlGen);
                    rows.Clear();
                }
            }
        }
        _repository.UpsertEntries(rows, crawlGen);

        foreach (var chunk in sizes.Build(_driveRoot, computedUtc, _dirs).Chunk(UpsertChunk))
            _dirSizeRepository.UpsertMany(chunk);
        return true;
    }

    /// <summary>Reads a file's real length from disk — used only for the rare fragmented files
    /// whose size the base MFT record can't report. Best-effort; a vanished file counts as 0.</summary>
    private long StatFileSize(in MftFileRecord rec)
    {
        string parentPath;
        if (rec.ParentRecordNumber == NtfsLayout.RootRecordNumber)
            parentPath = _driveRoot;
        else if (_dirs.TryGetValue(rec.ParentRecordNumber, out var parent))
            parentPath = parent.Path;
        else
            return 0;

        var path = parentPath.EndsWith('\\') ? parentPath + rec.Name : parentPath + '\\' + rec.Name;
        try
        {
            return Math.Max(0, new FileInfo(path).Length);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return 0;
        }
    }

    /// <summary>Resolves a raw record's canonical key + effective-hidden from the resident
    /// directory map. Directories look up their own resolved path; files hang off their parent.</summary>
    private bool TryResolveRecord(in MftFileRecord rec, out string key, out bool hidden)
    {
        key = "";
        hidden = false;
        if (rec.IsDirectory)
        {
            if (!_dirs.TryGetValue(rec.RecordNumber, out var dir))
                return false;
            key = dir.Path.ToUpperInvariant();
            hidden = dir.Hidden;
            return true;
        }

        string parentPath;
        bool parentHidden;
        if (rec.ParentRecordNumber == NtfsLayout.RootRecordNumber)
        {
            parentPath = _driveRoot;
            parentHidden = false;
        }
        else if (_dirs.TryGetValue(rec.ParentRecordNumber, out var parent))
        {
            parentPath = parent.Path;
            parentHidden = parent.Hidden;
        }
        else
        {
            return false;
        }

        var display = parentPath.EndsWith('\\') ? parentPath + rec.Name : parentPath + '\\' + rec.Name;
        key = display.ToUpperInvariant();
        hidden = parentHidden || rec.Hidden;
        return true;
    }

    /// <summary>Fallback build via FSCTL_ENUM_USN_DATA — names only (size 0, no timestamp);
    /// the DFS scanner still provides sizes for these volumes on demand.</summary>
    private void BuildFromUsnEnum(CancellationToken ct, long crawlGen)
    {
        var map = EnumerateMft(ct);
        if (ct.IsCancellationRequested) return;

        _dirs.Clear();
        var rows = new List<FsEntryRow>(UpsertChunk);
        foreach (var (frn, node) in map)
        {
            ct.ThrowIfCancellationRequested();
            if (frn == NtfsLayout.RootRecordNumber || node.Name is "." or "..")
                continue;
            if (!MftPathBuilder.TryResolve(map, frn, _driveRoot, _dirs, out var display, out var hidden, NtfsLayout.RootRecordNumber))
                continue;

            rows.Add(new FsEntryRow(display.ToUpperInvariant(), node.Name, node.IsDirectory, 0, DateTime.MinValue, hidden));
            if (rows.Count >= UpsertChunk)
            {
                _repository.UpsertEntries(rows, crawlGen);
                rows.Clear();
            }
        }
        _repository.UpsertEntries(rows, crawlGen);
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
                break; // ERROR_HANDLE_EOF is the normal end of the table
            if (bytes <= sizeof(ulong))
                break;

            foreach (var rec in UsnRecordParser.Parse(buffer, sizeof(ulong), bytes))
                map[NtfsLayout.RecordNumber(rec.FileReferenceNumber)] =
                    new MftNode(rec.Name, NtfsLayout.RecordNumber(rec.ParentFileReferenceNumber), rec.IsDirectory, rec.IsHidden);

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
                    _pendingRenames[NtfsLayout.RecordNumber(rec.FileReferenceNumber)] = oldPath;
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
            RemoveDirSubtree(NtfsLayout.RecordNumber(rec.FileReferenceNumber), key);
    }

    private void ApplyRename(UsnRecord rec)
    {
        if (!TryResolvePath(rec, out var newDisplay, out var newKey))
            return;

        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (_pendingRenames.Remove(NtfsLayout.RecordNumber(rec.FileReferenceNumber), out var oldPath))
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

        var hidden = ParentHidden(NtfsLayout.RecordNumber(rec.ParentFileReferenceNumber)) || rec.IsHidden;
        var (size, modified) = StatBestEffort(display, rec.IsDirectory);
        var crawlGen = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        _repository.UpsertEntries(new[] { new FsEntryRow(key, rec.Name, rec.IsDirectory, size, modified, hidden) }, crawlGen);

        if (rec.IsDirectory)
            _dirs[NtfsLayout.RecordNumber(rec.FileReferenceNumber)] = (display, hidden);
    }

    /// <summary>Resolves the display path (and canonical key) of a change record from its
    /// parent directory in the resident map. Fails when the parent is unknown (skips the
    /// record — best-effort, matching the crawl fallback's tolerance).</summary>
    private bool TryResolvePath(UsnRecord rec, out string display, out string key)
    {
        display = "";
        key = "";
        var parentRecord = NtfsLayout.RecordNumber(rec.ParentFileReferenceNumber);
        if (parentRecord == NtfsLayout.RootRecordNumber)
        {
            display = _driveRoot + rec.Name;
        }
        else if (_dirs.TryGetValue(parentRecord, out var parent))
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

    private bool ParentHidden(ulong parentRecord) =>
        parentRecord != NtfsLayout.RootRecordNumber
        && _dirs.TryGetValue(parentRecord, out var parent) && parent.Hidden;

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
    private void RemoveDirSubtree(ulong recordNumber, string key)
    {
        _dirs.Remove(recordNumber);
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
        var hidden = ParentHidden(NtfsLayout.RecordNumber(rec.ParentFileReferenceNumber)) || rec.IsHidden;
        _dirs[NtfsLayout.RecordNumber(rec.FileReferenceNumber)] = (newDisplay, hidden);

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
