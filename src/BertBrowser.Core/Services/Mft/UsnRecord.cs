using System.Buffers.Binary;

namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// One parsed USN_RECORD_V2 — the shape both the initial MFT enumeration
/// (FSCTL_ENUM_USN_DATA) and the live journal tail (FSCTL_READ_USN_JOURNAL) return.
/// <see cref="Reason"/>, <see cref="Usn"/> and <see cref="TimestampUtc"/> are only meaningful for
/// journal records; enumeration leaves them 0 / <see cref="DateTime.MinValue"/>.
/// </summary>
internal readonly record struct UsnRecord(
    ulong FileReferenceNumber,
    ulong ParentFileReferenceNumber,
    uint Reason,
    FileAttributes Attributes,
    string Name,
    long Usn = 0,
    DateTime TimestampUtc = default)
{
    public bool IsDirectory => (Attributes & FileAttributes.Directory) != 0;
    public bool IsHidden => (Attributes & FileAttributes.Hidden) != 0;
    public bool IsReparsePoint => (Attributes & FileAttributes.ReparsePoint) != 0;
}

/// <summary>
/// Decodes packed USN_RECORD_V2 entries out of the raw byte buffer a USN FSCTL fills.
/// Pure and allocation-light (only the file-name strings), so it is unit-tested against
/// canned buffers. Records with an unexpected major version (e.g. V3/V4 on ReFS) are
/// skipped by record length rather than misparsed — we only ever target NTFS volumes.
/// </summary>
internal static class UsnRecordParser
{
    // USN_RECORD_V2 field offsets (bytes from the start of each record).
    private const int RecordLengthOffset = 0;
    private const int MajorVersionOffset = 4;
    private const int FileReferenceNumberOffset = 8;
    private const int ParentFileReferenceNumberOffset = 16;
    private const int UsnOffset = 24;
    private const int TimeStampOffset = 32;
    private const int ReasonOffset = 40;
    private const int FileAttributesOffset = 52;
    private const int FileNameLengthOffset = 56;
    private const int FileNameOffsetOffset = 58;
    private const int MinRecordLength = 60;

    /// <summary>
    /// Enumerates the records contained in <paramref name="buffer"/>[<paramref name="start"/>..
    /// <paramref name="end"/>]. FSCTL_ENUM_USN_DATA / FSCTL_READ_USN_JOURNAL prefix their
    /// output with an 8-byte "next reference / next USN" cursor, so callers pass
    /// <paramref name="start"/> = 8 and <paramref name="end"/> = bytesReturned.
    /// </summary>
    public static IEnumerable<UsnRecord> Parse(byte[] buffer, int start, int end)
    {
        var offset = start;
        while (offset + MinRecordLength <= end)
        {
            var span = buffer.AsSpan(offset, end - offset);
            var recordLength = (int)BinaryPrimitives.ReadUInt32LittleEndian(span[RecordLengthOffset..]);
            if (recordLength < MinRecordLength || offset + recordLength > end)
                yield break; // truncated / corrupt tail — stop rather than misread

            var major = BinaryPrimitives.ReadUInt16LittleEndian(span[MajorVersionOffset..]);
            if (major == 2)
            {
                var frn = BinaryPrimitives.ReadUInt64LittleEndian(span[FileReferenceNumberOffset..]);
                var parentFrn = BinaryPrimitives.ReadUInt64LittleEndian(span[ParentFileReferenceNumberOffset..]);
                var usn = BinaryPrimitives.ReadInt64LittleEndian(span[UsnOffset..]);
                var stamp = FileTimeToUtc(BinaryPrimitives.ReadInt64LittleEndian(span[TimeStampOffset..]));
                var reason = BinaryPrimitives.ReadUInt32LittleEndian(span[ReasonOffset..]);
                var attrs = (FileAttributes)BinaryPrimitives.ReadUInt32LittleEndian(span[FileAttributesOffset..]);
                var nameLength = BinaryPrimitives.ReadUInt16LittleEndian(span[FileNameLengthOffset..]);
                var nameOffset = BinaryPrimitives.ReadUInt16LittleEndian(span[FileNameOffsetOffset..]);

                if (nameOffset + nameLength <= recordLength && nameLength > 0)
                {
                    var name = System.Text.Encoding.Unicode.GetString(
                        span.Slice(nameOffset, nameLength));
                    yield return new UsnRecord(frn, parentFrn, reason, attrs, name, usn, stamp);
                }
            }

            offset += recordLength;
        }
    }

    /// <summary>The same guard <c>MftReader</c> applies: an unset FILETIME is unknown, not 1601.</summary>
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
