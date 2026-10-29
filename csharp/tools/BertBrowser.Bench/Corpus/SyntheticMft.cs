using System.Buffers.Binary;
using System.Text;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Mft;

namespace BertBrowser.Bench.Corpus;

/// <summary>
/// The invented disk as its master file table would describe it: one record per entry, knowing only
/// its own name and its parent's record number — which is exactly what the path builder and the
/// size roll-up have to work from.
/// </summary>
internal static class SyntheticMft
{
    /// <summary>Records start here; below 16 are NTFS's reserved metafiles, which the reader skips.</summary>
    private const ulong FirstRecord = 64;

    public static (List<MftFileRecord> Records, Dictionary<ulong, MftNode> Directories) Build(SyntheticPaths model)
    {
        var rootKey = PathKey.Canonicalize(SyntheticPaths.Drive);
        var numbers = new Dictionary<string, ulong>(StringComparer.Ordinal) { [rootKey] = NtfsLayout.RootRecordNumber };
        var records = new List<MftFileRecord>(model.Rows.Count);
        var directories = new Dictionary<ulong, MftNode>(model.Directories.Count);
        var next = FirstRecord;

        foreach (var row in model.Rows)
        {
            var parentKey = Path.GetDirectoryName(row.PathKey) ?? rootKey;
            var parent = numbers[parentKey];
            var number = next++;
            if (row.IsDirectory)
            {
                numbers[row.PathKey] = number;
                directories[number] = new MftNode(row.Name, parent, true, row.Attributes);
            }

            records.Add(new MftFileRecord(
                number, parent, row.Name, row.IsDirectory, row.Attributes, row.SizeBytes, row.ModifiedUtc, row.CreatedUtc));
        }

        return (records, directories);
    }

    /// <summary>
    /// A valid 1 KB FILE record for <paramref name="record"/>, laid out as NTFS would: header,
    /// <c>$STANDARD_INFORMATION</c>, one Win32 <c>$FILE_NAME</c>, a non-resident <c>$DATA</c> for a
    /// file, and the end marker. The same shape <c>NtfsParsingTests</c> builds.
    /// </summary>
    public static byte[] FileRecordBytes(in MftFileRecord record)
    {
        var rec = new byte[1024];
        Encoding.ASCII.GetBytes("FILE").CopyTo(rec, 0);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqOffset), 0x30);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqSize), 3);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecFirstAttributeOffset), 0x38);
        var flags = NtfsLayout.RecFlagInUse | (record.IsDirectory ? NtfsLayout.RecFlagDirectory : 0);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecFlags), (ushort)flags);

        var off = 0x38;
        off += WriteStdInfo(rec, off, record.ModifiedUtc.ToFileTimeUtc(), (uint)record.Attributes, record.CreatedUtc.ToFileTimeUtc());
        off += WriteFileName(rec, off, record.ParentRecordNumber, record.Name, ns: 1);
        if (!record.IsDirectory)
            off += WriteDataNonResident(rec, off, record.Size);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off), NtfsLayout.AttrTypeEnd);
        return rec;
    }

    private static int WriteResidentHeader(byte[] rec, int off, uint type, int contentLen)
    {
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x00), type);
        var len = Align8(0x18 + contentLen);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x04), (uint)len);
        rec[off + 0x08] = 0;
        rec[off + 0x09] = 0;
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x10), (uint)contentLen);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(off + 0x14), 0x18);
        return len;
    }

    private static int WriteStdInfo(byte[] rec, int off, long modifiedFileTime, uint fileAttrs, long createdFileTime)
    {
        const int contentLen = 0x30;
        var len = WriteResidentHeader(rec, off, NtfsLayout.AttrStandardInformation, contentLen);
        var c = off + 0x18;
        BinaryPrimitives.WriteInt64LittleEndian(rec.AsSpan(c + NtfsLayout.StdInfoCreated), createdFileTime);
        BinaryPrimitives.WriteInt64LittleEndian(rec.AsSpan(c + NtfsLayout.StdInfoModified), modifiedFileTime);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(c + NtfsLayout.StdInfoFileAttributes), fileAttrs);
        return len;
    }

    private static int WriteFileName(byte[] rec, int off, ulong parentRef, string name, byte ns)
    {
        var nameBytes = Encoding.Unicode.GetBytes(name);
        var contentLen = NtfsLayout.FileNameName + nameBytes.Length;
        var len = WriteResidentHeader(rec, off, NtfsLayout.AttrFileName, contentLen);
        var c = off + 0x18;
        BinaryPrimitives.WriteUInt64LittleEndian(rec.AsSpan(c + NtfsLayout.FileNameParentRef), parentRef);
        rec[c + NtfsLayout.FileNameLength] = (byte)name.Length;
        rec[c + NtfsLayout.FileNameNamespace] = ns;
        nameBytes.CopyTo(rec, c + NtfsLayout.FileNameName);
        return len;
    }

    private static int WriteDataNonResident(byte[] rec, int off, long realSize)
    {
        const int len = 0x48;
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x00), NtfsLayout.AttrData);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x04), len);
        rec[off + 0x08] = 1;
        rec[off + 0x09] = 0;
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(off + NtfsLayout.AttrNonResDataRunsOffset), 0x40);
        BinaryPrimitives.WriteInt64LittleEndian(rec.AsSpan(off + NtfsLayout.AttrNonResRealSize), realSize);
        return len;
    }

    private static int Align8(int value) => (value + 7) & ~7;
}
