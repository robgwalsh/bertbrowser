using System.Buffers.Binary;
using System.Text;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class NtfsParsingTests
{
    // --- Data-run (runlist) decoding ---

    [Fact]
    public void DecodeDataRuns_SingleRun()
    {
        // header 0x21: 1 length byte, 2 offset bytes. length=0x18=24, lcn=0x5634=22068.
        var runs = NtfsLayout.DecodeDataRuns(new byte[] { 0x21, 0x18, 0x34, 0x56, 0x00 });

        var run = Assert.Single(runs);
        Assert.Equal(22068, run.StartLcn);
        Assert.Equal(24, run.LengthClusters);
    }

    [Fact]
    public void DecodeDataRuns_MultiRunWithNegativeDelta()
    {
        // Second run's offset 0xE0 is a signed byte (-32): lcn = 22068 - 32 = 22036.
        var runs = NtfsLayout.DecodeDataRuns(new byte[] { 0x21, 0x18, 0x34, 0x56, 0x11, 0x10, 0xE0, 0x00 });

        Assert.Equal(2, runs.Count);
        Assert.Equal(22068, runs[0].StartLcn);
        Assert.Equal(24, runs[0].LengthClusters);
        Assert.Equal(22036, runs[1].StartLcn);
        Assert.Equal(16, runs[1].LengthClusters);
    }

    [Fact]
    public void DecodeDataRuns_SparseHoleHasNoLcn()
    {
        // header 0x01: 1 length byte, 0 offset bytes -> sparse.
        var runs = NtfsLayout.DecodeDataRuns(new byte[] { 0x01, 0x05, 0x00 });

        var run = Assert.Single(runs);
        Assert.Equal(-1, run.StartLcn);
        Assert.Equal(5, run.LengthClusters);
    }

    // --- Boot sector ---

    [Fact]
    public void ParseBootSector_ReadsGeometry()
    {
        var boot = new byte[512];
        Encoding.ASCII.GetBytes("NTFS    ").CopyTo(boot, 3);
        BinaryPrimitives.WriteUInt16LittleEndian(boot.AsSpan(0x0B), 512);   // bytes/sector
        boot[0x0D] = 8;                                                     // sectors/cluster -> 4096
        BinaryPrimitives.WriteInt64LittleEndian(boot.AsSpan(0x30), 786432); // $MFT start LCN
        boot[0x40] = unchecked((byte)(sbyte)-10);                          // record size 2^10 = 1024

        Assert.True(MftReader.TryParseBootSector(boot, out var bps, out var bpc, out var mftLcn, out var bpr));
        Assert.Equal(512, bps);
        Assert.Equal(4096, bpc);
        Assert.Equal(786432, mftLcn);
        Assert.Equal(1024, bpr);
    }

    [Fact]
    public void ParseBootSector_RejectsNonNtfs()
    {
        var boot = new byte[512];
        Encoding.ASCII.GetBytes("FAT32   ").CopyTo(boot, 3);
        Assert.False(MftReader.TryParseBootSector(boot, out _, out _, out _, out _));
    }

    // --- Update-sequence-array fixup ---

    [Fact]
    public void ApplyFixup_RestoresSectorTails()
    {
        var rec = new byte[1024];
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqOffset), 0x30); // USA at 0x30
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqSize), 3);       // USN + 2 fixups
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(0x30), 0xAAAA); // USN signature
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(0x32), 0x1234); // real bytes for sector 0 tail
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(0x34), 0x5678); // real bytes for sector 1 tail

        MftReader.ApplyFixup(rec, 0, 1024, 512);

        Assert.Equal(0x34, rec[510]); Assert.Equal(0x12, rec[511]);   // 0x1234 LE
        Assert.Equal(0x78, rec[1022]); Assert.Equal(0x56, rec[1023]); // 0x5678 LE
    }

    // --- FILE record parsing ---

    private static readonly DateTime Modified = new(2020, 1, 2, 3, 4, 5, DateTimeKind.Utc);

    [Fact]
    public void ParseFileRecord_ReadsNameParentSizeAndDate()
    {
        var rec = BuildRecord(isDir: false, hidden: false, size: 1_000_000,
            fileNames: new[] { (name: "report.txt", ns: (byte)1, parent: 5UL) });

        Assert.True(MftReader.TryParseFileRecord(rec, recordNumber: 40, recordLength: 1024, bytesPerSector: 512, out var parsed));
        Assert.Equal("report.txt", parsed.Name);
        Assert.Equal(5UL, parsed.ParentRecordNumber);
        Assert.False(parsed.IsDirectory);
        Assert.False(parsed.Hidden);
        Assert.Equal(1_000_000, parsed.Size);
        Assert.Equal(Modified, parsed.ModifiedUtc);
    }

    [Fact]
    public void ParseFileRecord_PrefersWin32NameOverDos()
    {
        // DOS 8.3 alias listed first; the Win32 name must win.
        var rec = BuildRecord(isDir: false, hidden: false, size: 10,
            fileNames: new[] { (name: "REPORT~1.TXT", ns: (byte)2, parent: 5UL), (name: "report.txt", ns: (byte)1, parent: 5UL) });

        Assert.True(MftReader.TryParseFileRecord(rec, 40, 1024, 512, out var parsed));
        Assert.Equal("report.txt", parsed.Name);
    }

    [Fact]
    public void ParseFileRecord_ReadsHiddenAndDirectoryFlags()
    {
        var rec = BuildRecord(isDir: true, hidden: true, size: 0,
            fileNames: new[] { (name: "Secret", ns: (byte)1, parent: 5UL) });

        Assert.True(MftReader.TryParseFileRecord(rec, 40, 1024, 512, out var parsed));
        Assert.True(parsed.IsDirectory);
        Assert.True(parsed.Hidden);
        Assert.Equal(0, parsed.Size); // directories carry no $DATA size
    }

    [Fact]
    public void ParseFileRecord_SkipsReservedMetafiles()
    {
        var rec = BuildRecord(false, false, 100, new[] { (name: "$MFT", ns: (byte)1, parent: 5UL) });
        Assert.False(MftReader.TryParseFileRecord(rec, recordNumber: 0, 1024, 512, out _));
    }

    [Fact]
    public void ParseFileRecord_NoBaseDataYieldsUnknownSize()
    {
        // A heavily-fragmented file's base record has no unnamed $DATA (it lives in extension
        // records) — the parser signals -1 so the caller stats it from disk.
        var rec = BuildRecord(isDir: false, hidden: false, size: 0,
            fileNames: new[] { (name: "fragmented.pack", ns: (byte)1, parent: 5UL) }, withData: false);

        Assert.True(MftReader.TryParseFileRecord(rec, 40, 1024, 512, out var parsed));
        Assert.Equal(-1, parsed.Size);
    }

    /// <summary>Assembles a minimal-but-valid FILE record: header + $STANDARD_INFORMATION +
    /// one or more $FILE_NAME + a non-resident $DATA + end marker. USA words are zero (tails
    /// already zero) so the fixup is a harmless no-op over the attributes.</summary>
    private static byte[] BuildRecord(bool isDir, bool hidden, long size, (string name, byte ns, ulong parent)[] fileNames, bool withData = true)
    {
        var rec = new byte[1024];
        Encoding.ASCII.GetBytes("FILE").CopyTo(rec, 0);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqOffset), 0x30);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecUpdateSeqSize), 3);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecFirstAttributeOffset), 0x38);
        var flags = NtfsLayout.RecFlagInUse | (isDir ? NtfsLayout.RecFlagDirectory : 0);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(NtfsLayout.RecFlags), (ushort)flags);

        var off = 0x38;
        off += WriteStdInfo(rec, off, Modified.ToFileTimeUtc(), hidden ? NtfsLayout.FileAttributeHidden : 0);
        foreach (var (name, ns, parent) in fileNames)
            off += WriteFileName(rec, off, parent, name, ns);
        if (!isDir && withData)
            off += WriteDataNonResident(rec, off, size);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off), NtfsLayout.AttrTypeEnd);
        return rec;
    }

    private static int WriteResidentHeader(byte[] rec, int off, uint type, int contentLen)
    {
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x00), type);
        var len = Align8(0x18 + contentLen);
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x04), (uint)len);
        rec[off + 0x08] = 0; // resident
        rec[off + 0x09] = 0; // unnamed
        BinaryPrimitives.WriteUInt32LittleEndian(rec.AsSpan(off + 0x10), (uint)contentLen);
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(off + 0x14), 0x18); // value offset
        return len;
    }

    private static int WriteStdInfo(byte[] rec, int off, long modifiedFileTime, uint fileAttrs)
    {
        const int contentLen = 0x30;
        var len = WriteResidentHeader(rec, off, NtfsLayout.AttrStandardInformation, contentLen);
        var c = off + 0x18;
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
        rec[off + 0x08] = 1; // non-resident
        rec[off + 0x09] = 0; // unnamed
        BinaryPrimitives.WriteUInt16LittleEndian(rec.AsSpan(off + NtfsLayout.AttrNonResDataRunsOffset), 0x40);
        BinaryPrimitives.WriteInt64LittleEndian(rec.AsSpan(off + NtfsLayout.AttrNonResRealSize), realSize);
        return len;
    }

    private static int Align8(int value) => (value + 7) & ~7;
}
