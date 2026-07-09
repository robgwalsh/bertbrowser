namespace BertBrowser.Core.Services.Mft;

/// <summary>One extent of a non-resident attribute: a run of <see cref="LengthClusters"/>
/// clusters starting at logical cluster <see cref="StartLcn"/>. A sparse run (a hole) has
/// <see cref="StartLcn"/> = -1.</summary>
internal readonly record struct DataRun(long StartLcn, long LengthClusters);

/// <summary>
/// On-disk NTFS structure offsets/constants and the data-run (runlist) decoder — everything
/// needed to walk the raw <c>$MFT</c>. Field offsets are from the NTFS on-disk format; see
/// <see cref="MftReader"/> for how they're used. The runlist decoder is pure and unit-tested.
/// </summary>
internal static class NtfsLayout
{
    // --- Boot sector (volume boot record) ---
    public const int BootBytesPerSector = 0x0B;                 // u16
    public const int BootSectorsPerCluster = 0x0D;              // u8
    public const int BootMftStartLcn = 0x30;                    // i64
    public const int BootClustersPerFileRecord = 0x40;          // i8 (signed)

    // --- FILE record header ---
    public const uint FileSignature = 0x454C4946;               // "FILE" little-endian
    public const int RecUpdateSeqOffset = 0x04;                 // u16
    public const int RecUpdateSeqSize = 0x06;                   // u16 (count of u16 words)
    public const int RecFirstAttributeOffset = 0x14;            // u16
    public const int RecFlags = 0x16;                           // u16
    public const int RecFlagInUse = 0x0001;
    public const int RecFlagDirectory = 0x0002;

    // --- Attribute header (common) ---
    public const uint AttrTypeEnd = 0xFFFFFFFF;
    public const int AttrType = 0x00;                           // u32
    public const int AttrLength = 0x04;                         // u32 (advance by this)
    public const int AttrNonResidentFlag = 0x08;               // u8
    // Resident attribute:
    public const int AttrResValueLength = 0x10;                // u32
    public const int AttrResValueOffset = 0x14;                // u16
    // Non-resident attribute:
    public const int AttrNonResDataRunsOffset = 0x20;          // u16
    public const int AttrNonResRealSize = 0x30;                // u64

    // --- Attribute type codes ---
    public const uint AttrStandardInformation = 0x10;
    public const uint AttrFileName = 0x30;
    public const uint AttrData = 0x80;

    // --- $STANDARD_INFORMATION content ---
    public const int StdInfoModified = 0x08;                    // u64 FILETIME
    public const int StdInfoFileAttributes = 0x20;             // u32

    // --- $FILE_NAME content ---
    public const int FileNameParentRef = 0x00;                 // u64 (low 48 bits = record number)
    public const int FileNameRealSize = 0x30;                  // u64
    public const int FileNameFlags = 0x38;                     // u32
    public const int FileNameLength = 0x40;                    // u8 (characters)
    public const int FileNameNamespace = 0x41;                 // u8
    public const int FileNameName = 0x42;                      // WCHAR[]

    // File-name namespaces (higher = more preferred when a record has several names).
    public const byte NamespaceDos = 2;

    // FILE_ATTRIBUTE bits we read.
    public const uint FileAttributeHidden = 0x0002;
    public const uint FileAttributeDirectory = 0x0010;

    /// <summary>The 48-bit record number of the root directory ("."), always 5.</summary>
    public const ulong RootRecordNumber = 5;

    /// <summary>Low 48 bits of an NTFS file reference are the MFT record number; the high 16
    /// are a reuse sequence number we don't need for path building.</summary>
    public static ulong RecordNumber(ulong fileReference) => fileReference & 0x0000_FFFF_FFFF_FFFFUL;

    /// <summary>
    /// Decodes an NTFS data-run list into absolute cluster runs. Each run is a header byte
    /// (low nibble = length field size, high nibble = LCN-delta field size) followed by an
    /// unsigned length and a signed LCN delta from the previous run's start. A zero header
    /// ends the list; a zero delta-size marks a sparse hole (StartLcn = -1).
    /// </summary>
    public static List<DataRun> DecodeDataRuns(ReadOnlySpan<byte> runList)
    {
        var runs = new List<DataRun>();
        long lcn = 0;
        var i = 0;
        while (i < runList.Length)
        {
            var header = runList[i++];
            if (header == 0)
                break;

            var lengthSize = header & 0x0F;
            var offsetSize = (header >> 4) & 0x0F;
            if (lengthSize == 0 || i + lengthSize + offsetSize > runList.Length)
                break; // malformed / truncated

            var length = ReadLittleEndianUnsigned(runList.Slice(i, lengthSize));
            i += lengthSize;

            if (offsetSize == 0)
            {
                runs.Add(new DataRun(-1, length)); // sparse hole
                continue;
            }

            lcn += ReadLittleEndianSigned(runList.Slice(i, offsetSize));
            i += offsetSize;
            runs.Add(new DataRun(lcn, length));
        }
        return runs;
    }

    private static long ReadLittleEndianUnsigned(ReadOnlySpan<byte> bytes)
    {
        long value = 0;
        for (var k = bytes.Length - 1; k >= 0; k--)
            value = (value << 8) | bytes[k];
        return value;
    }

    private static long ReadLittleEndianSigned(ReadOnlySpan<byte> bytes)
    {
        var value = ReadLittleEndianUnsigned(bytes);
        var bits = bytes.Length * 8;
        if (bits < 64 && (value & (1L << (bits - 1))) != 0)
            value |= -1L << bits; // sign-extend
        return value;
    }
}
