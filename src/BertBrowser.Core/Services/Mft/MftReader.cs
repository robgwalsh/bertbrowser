using System.Buffers.Binary;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace BertBrowser.Core.Services.Mft;

/// <summary>One file/directory read straight from an NTFS FILE record. Unlike the USN
/// enumeration, this carries the real <see cref="Size"/> (unnamed <c>$DATA</c> real size)
/// and <see cref="ModifiedUtc"/>, which is what makes instant directory sizing possible.
/// <see cref="Size"/> is -1 for a file whose <c>$DATA</c> lives in extension records (the
/// base record can't report its size); the caller stats those from disk. Directories are 0.
/// Keyed by 48-bit MFT record numbers (root = 5).</summary>
internal readonly record struct MftFileRecord(
    ulong RecordNumber,
    ulong ParentRecordNumber,
    string Name,
    bool IsDirectory,
    bool Hidden,
    long Size,
    DateTime ModifiedUtc);

/// <summary>
/// Reads the raw NTFS <c>$MFT</c> from a volume handle: parses the boot sector for geometry,
/// resolves the <c>$MFT</c>'s own (fragmented) extent map from record 0, then streams every
/// in-use FILE record — applying the update-sequence fixup and pulling name, parent, size,
/// timestamp and attributes. Volume reads are issued cluster-aligned (a hard requirement for
/// raw <c>\\.\C:</c> access) via <see cref="RandomAccess"/>. The FILE-record and boot-sector
/// parsers are split into pure static helpers so they can be unit-tested against canned bytes.
/// </summary>
internal sealed class MftReader
{
    private const int ChunkTargetBytes = 4 * 1024 * 1024;

    private readonly SafeFileHandle _volume;

    private int _bytesPerSector;
    private int _bytesPerCluster;
    private long _mftStartLcn;
    private int _bytesPerFileRecord;

    public MftReader(SafeFileHandle volume) => _volume = volume;

    /// <summary>
    /// Streams every in-use file/directory record to <paramref name="onRecord"/>. Returns false
    /// (rather than throwing) when the volume geometry can't be parsed, so the caller can fall
    /// back to the USN enumeration. Reserved system metafiles (record &lt; 16, e.g. <c>$MFT</c>,
    /// <c>$LogFile</c>, the root's ".") are skipped so sizes match the user-visible tree.
    /// </summary>
    public bool TryReadAll(CancellationToken ct, Action<MftFileRecord> onRecord)
    {
        var boot = new byte[8192];
        ReadExact(0, boot);
        if (!TryParseBootSector(boot, out _bytesPerSector, out _bytesPerCluster, out _mftStartLcn, out _bytesPerFileRecord))
            return false;

        // Record 0 is the $MFT itself; its unnamed $DATA runlist is the full extent map.
        var firstCluster = new byte[Math.Max(_bytesPerCluster, _bytesPerFileRecord)];
        ReadExact(_mftStartLcn * _bytesPerCluster, firstCluster);
        ApplyFixup(firstCluster, 0, _bytesPerFileRecord, _bytesPerSector);
        var mftRuns = ReadMftDataRuns(firstCluster);
        if (mftRuns.Count == 0)
            return false;

        var chunkClusters = Math.Max(1, ChunkTargetBytes / _bytesPerCluster);
        var chunk = new byte[chunkClusters * _bytesPerCluster];
        var pending = new byte[_bytesPerFileRecord];
        var pendingLen = 0;
        ulong recordIndex = 0;

        foreach (var run in mftRuns)
        {
            if (run.StartLcn < 0)
            {
                // Sparse hole in the $MFT (rare): advance the record counter past it.
                recordIndex += (ulong)(run.LengthClusters * _bytesPerCluster / _bytesPerFileRecord);
                continue;
            }

            var clustersLeft = run.LengthClusters;
            var lcn = run.StartLcn;
            while (clustersLeft > 0)
            {
                ct.ThrowIfCancellationRequested();
                var take = (int)Math.Min(clustersLeft, chunkClusters);
                var bytes = take * _bytesPerCluster;
                ReadExact(lcn * _bytesPerCluster, chunk.AsSpan(0, bytes));
                FeedRecords(chunk.AsSpan(0, bytes), pending, ref pendingLen, ref recordIndex, onRecord);
                lcn += take;
                clustersLeft -= take;
            }
        }
        return true;
    }

    /// <summary>Slices whole FILE records out of a freshly-read chunk, carrying a partial record
    /// across chunk/run boundaries (needed only when a record spans clusters — small clusters).</summary>
    private void FeedRecords(ReadOnlySpan<byte> data, byte[] pending, ref int pendingLen, ref ulong recordIndex, Action<MftFileRecord> onRecord)
    {
        var pos = 0;
        if (pendingLen > 0)
        {
            var need = _bytesPerFileRecord - pendingLen;
            data[..need].CopyTo(pending.AsSpan(pendingLen));
            ParseAndEmit(pending, 0, recordIndex++, onRecord);
            pendingLen = 0;
            pos = need;
        }

        while (pos + _bytesPerFileRecord <= data.Length)
        {
            ParseAndEmit(data.Slice(pos, _bytesPerFileRecord), recordIndex++, onRecord);
            pos += _bytesPerFileRecord;
        }

        var leftover = data.Length - pos;
        if (leftover > 0)
        {
            data[pos..].CopyTo(pending);
            pendingLen = leftover;
        }
    }

    private void ParseAndEmit(ReadOnlySpan<byte> record, ulong recordNumber, Action<MftFileRecord> onRecord)
    {
        var owned = record.ToArray(); // fixup mutates in place; don't touch the shared chunk buffer twice
        if (TryParseFileRecord(owned, recordNumber, _bytesPerFileRecord, _bytesPerSector, out var parsed))
            onRecord(parsed);
    }

    /// <summary>Overload for the carried-over <c>pending</c> buffer (already a private array).</summary>
    private void ParseAndEmit(byte[] record, int offset, ulong recordNumber, Action<MftFileRecord> onRecord)
    {
        if (TryParseFileRecord(record.AsSpan(offset, _bytesPerFileRecord).ToArray(), recordNumber, _bytesPerFileRecord, _bytesPerSector, out var parsed))
            onRecord(parsed);
    }

    private List<DataRun> ReadMftDataRuns(byte[] record)
    {
        // Walk record 0's attributes for the unnamed non-resident $DATA and decode its runlist.
        var first = BinaryPrimitives.ReadUInt16LittleEndian(record.AsSpan(NtfsLayout.RecFirstAttributeOffset));
        var offset = (int)first;
        while (offset + 8 <= record.Length)
        {
            var type = BinaryPrimitives.ReadUInt32LittleEndian(record.AsSpan(offset + NtfsLayout.AttrType));
            if (type == NtfsLayout.AttrTypeEnd)
                break;
            var length = (int)BinaryPrimitives.ReadUInt32LittleEndian(record.AsSpan(offset + NtfsLayout.AttrLength));
            if (length <= 0 || offset + length > record.Length)
                break;

            var nameLen = record[offset + 9];
            var nonResident = record[offset + NtfsLayout.AttrNonResidentFlag] != 0;
            if (type == NtfsLayout.AttrData && nameLen == 0 && nonResident)
            {
                var runsOffset = BinaryPrimitives.ReadUInt16LittleEndian(record.AsSpan(offset + NtfsLayout.AttrNonResDataRunsOffset));
                return NtfsLayout.DecodeDataRuns(record.AsSpan(offset + runsOffset, length - runsOffset));
            }
            offset += length;
        }
        return new List<DataRun>();
    }

    private void ReadExact(long offset, Span<byte> buffer)
    {
        var total = 0;
        while (total < buffer.Length)
        {
            var n = RandomAccess.Read(_volume, buffer[total..], offset + total);
            if (n == 0)
                throw new IOException($"Unexpected EOF reading volume at offset {offset + total}.");
            total += n;
        }
    }

    // --- Pure parsers (unit-tested) ---

    public static bool TryParseBootSector(
        ReadOnlySpan<byte> boot, out int bytesPerSector, out int bytesPerCluster, out long mftStartLcn, out int bytesPerFileRecord)
    {
        bytesPerSector = bytesPerCluster = bytesPerFileRecord = 0;
        mftStartLcn = 0;
        if (boot.Length < 512 || Encoding.ASCII.GetString(boot.Slice(3, 8)) != "NTFS    ")
            return false;

        bytesPerSector = BinaryPrimitives.ReadUInt16LittleEndian(boot[NtfsLayout.BootBytesPerSector..]);
        int sectorsPerCluster = boot[NtfsLayout.BootSectorsPerCluster];
        if (bytesPerSector < 256 || (bytesPerSector & (bytesPerSector - 1)) != 0 || sectorsPerCluster == 0)
            return false;

        bytesPerCluster = bytesPerSector * sectorsPerCluster;
        mftStartLcn = BinaryPrimitives.ReadInt64LittleEndian(boot[NtfsLayout.BootMftStartLcn..]);

        var raw = unchecked((sbyte)boot[NtfsLayout.BootClustersPerFileRecord]);
        bytesPerFileRecord = raw >= 0 ? raw * bytesPerCluster : 1 << (-raw);
        return bytesPerFileRecord >= 512 && mftStartLcn > 0;
    }

    /// <summary>Restores the last two bytes of each sector from the update-sequence array. NTFS
    /// stamps a signature word into those positions on disk to detect torn writes; without
    /// undoing it, any field straddling a sector boundary is corrupt.</summary>
    public static void ApplyFixup(Span<byte> record, int recordOffset, int recordLength, int bytesPerSector)
    {
        var usaOffset = recordOffset + BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(recordOffset + NtfsLayout.RecUpdateSeqOffset));
        var usaCount = BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(recordOffset + NtfsLayout.RecUpdateSeqSize));
        if (usaCount == 0)
            return;

        for (var sector = 0; sector < usaCount - 1; sector++)
        {
            var tail = recordOffset + (sector + 1) * bytesPerSector - 2;
            var usaEntry = usaOffset + 2 + sector * 2;
            if (tail + 2 > recordOffset + recordLength || usaEntry + 2 > record.Length)
                break;
            record[tail] = record[usaEntry];
            record[tail + 1] = record[usaEntry + 1];
        }
    }

    public static bool TryParseFileRecord(
        Span<byte> record, ulong recordNumber, int recordLength, int bytesPerSector, out MftFileRecord parsed)
    {
        parsed = default;
        if (recordNumber < 16) // reserved NTFS metafiles ($MFT, $LogFile, root ".", $Extend, …)
            return false;
        if (record.Length < recordLength || BinaryPrimitives.ReadUInt32LittleEndian(record) != NtfsLayout.FileSignature)
            return false;

        ApplyFixup(record, 0, recordLength, bytesPerSector);

        var flags = BinaryPrimitives.ReadUInt16LittleEndian(record[NtfsLayout.RecFlags..]);
        if ((flags & NtfsLayout.RecFlagInUse) == 0)
            return false; // deleted
        // An extension record (non-zero base file reference) — its attributes belong to a base
        // record we'll parse separately.
        if (BinaryPrimitives.ReadUInt64LittleEndian(record[0x20..]) != 0)
            return false;

        var isDirectory = (flags & NtfsLayout.RecFlagDirectory) != 0;
        string? name = null;
        byte chosenNamespace = 255;
        ulong parentRecord = 0;
        var modified = DateTime.MinValue;
        var hidden = false;
        long size = 0;
        var haveData = false;

        var offset = (int)BinaryPrimitives.ReadUInt16LittleEndian(record[NtfsLayout.RecFirstAttributeOffset..]);
        while (offset + 8 <= recordLength)
        {
            var type = BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + NtfsLayout.AttrType));
            if (type == NtfsLayout.AttrTypeEnd)
                break;
            var length = (int)BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + NtfsLayout.AttrLength));
            if (length < 8 || offset + length > recordLength)
                break;

            var nonResident = record[offset + NtfsLayout.AttrNonResidentFlag] != 0;
            var attrNameLen = record[offset + 9];

            if (type == NtfsLayout.AttrStandardInformation && !nonResident)
            {
                var content = offset + BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(offset + NtfsLayout.AttrResValueOffset));
                modified = FileTimeToUtc(BinaryPrimitives.ReadInt64LittleEndian(record.Slice(content + NtfsLayout.StdInfoModified)));
                hidden = (BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(content + NtfsLayout.StdInfoFileAttributes)) & NtfsLayout.FileAttributeHidden) != 0;
            }
            else if (type == NtfsLayout.AttrFileName && !nonResident)
            {
                var content = offset + BinaryPrimitives.ReadUInt16LittleEndian(record.Slice(offset + NtfsLayout.AttrResValueOffset));
                var ns = record[content + NtfsLayout.FileNameNamespace];
                // Prefer any non-DOS (Win32/POSIX) name over the 8.3 DOS alias.
                if (name is null || (chosenNamespace == NtfsLayout.NamespaceDos && ns != NtfsLayout.NamespaceDos))
                {
                    var nameChars = record[content + NtfsLayout.FileNameLength];
                    name = Encoding.Unicode.GetString(record.Slice(content + NtfsLayout.FileNameName, nameChars * 2));
                    parentRecord = NtfsLayout.RecordNumber(BinaryPrimitives.ReadUInt64LittleEndian(record.Slice(content + NtfsLayout.FileNameParentRef)));
                    chosenNamespace = ns;
                }
            }
            else if (type == NtfsLayout.AttrData && attrNameLen == 0 && !haveData)
            {
                size = nonResident
                    ? BinaryPrimitives.ReadInt64LittleEndian(record.Slice(offset + NtfsLayout.AttrNonResRealSize))
                    : BinaryPrimitives.ReadUInt32LittleEndian(record.Slice(offset + NtfsLayout.AttrResValueLength));
                haveData = true;
            }

            offset += length;
        }

        if (name is null)
            return false;

        // A file with no unnamed $DATA in its base record has that attribute in extension
        // records (heavy fragmentation) — signal "unknown" with -1 so the caller stats it.
        var fileSize = isDirectory ? 0 : haveData ? Math.Max(0, size) : -1;
        parsed = new MftFileRecord(recordNumber, parentRecord, name, isDirectory, hidden, fileSize, modified);
        return true;
    }

    private static DateTime FileTimeToUtc(long fileTime)
    {
        try
        {
            return fileTime <= 0 ? DateTime.MinValue : DateTime.FromFileTimeUtc(fileTime);
        }
        catch (ArgumentOutOfRangeException)
        {
            return DateTime.MinValue;
        }
    }
}
