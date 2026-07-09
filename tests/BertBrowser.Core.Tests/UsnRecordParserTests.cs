using System.Buffers.Binary;
using System.Text;
using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

public sealed class UsnRecordParserTests
{
    private const int NameOffset = 60;

    /// <summary>Builds one USN_RECORD_V2 with the given fields, 8-byte aligned like the OS.</summary>
    private static byte[] Record(ulong frn, ulong parentFrn, uint reason, uint attributes, string name, ushort major = 2)
    {
        var nameBytes = Encoding.Unicode.GetBytes(name);
        var length = (NameOffset + nameBytes.Length + 7) & ~7; // round up to 8
        var buf = new byte[length];
        var span = buf.AsSpan();
        BinaryPrimitives.WriteUInt32LittleEndian(span, (uint)length);        // RecordLength
        BinaryPrimitives.WriteUInt16LittleEndian(span[4..], major);          // MajorVersion
        BinaryPrimitives.WriteUInt64LittleEndian(span[8..], frn);
        BinaryPrimitives.WriteUInt64LittleEndian(span[16..], parentFrn);
        BinaryPrimitives.WriteUInt32LittleEndian(span[40..], reason);
        BinaryPrimitives.WriteUInt32LittleEndian(span[52..], attributes);
        BinaryPrimitives.WriteUInt16LittleEndian(span[56..], (ushort)nameBytes.Length);
        BinaryPrimitives.WriteUInt16LittleEndian(span[58..], NameOffset);
        nameBytes.CopyTo(span[NameOffset..]);
        return buf;
    }

    /// <summary>Prepends the 8-byte cursor an FSCTL output carries before its records.</summary>
    private static byte[] WithCursor(params byte[][] records)
    {
        var total = 8 + records.Sum(r => r.Length);
        var buf = new byte[total];
        var offset = 8;
        foreach (var r in records)
        {
            r.CopyTo(buf, offset);
            offset += r.Length;
        }
        return buf;
    }

    [Fact]
    public void Parse_ReadsFieldsAndName()
    {
        const uint directory = 0x10; // FILE_ATTRIBUTE_DIRECTORY
        var buffer = WithCursor(Record(0x111, 0x5, reason: 0x102, attributes: directory, name: "My Folder"));

        var rec = Assert.Single(UsnRecordParser.Parse(buffer, 8, buffer.Length));

        Assert.Equal(0x111UL, rec.FileReferenceNumber);
        Assert.Equal(0x5UL, rec.ParentFileReferenceNumber);
        Assert.Equal(0x102U, rec.Reason);
        Assert.Equal("My Folder", rec.Name);
        Assert.True(rec.IsDirectory);
        Assert.False(rec.IsHidden);
    }

    [Fact]
    public void Parse_ReadsMultipleRecords()
    {
        var buffer = WithCursor(
            Record(1, 5, 0, 0, "a.txt"),
            Record(2, 5, 0, 0x2 /* hidden */, "b.txt"));

        var recs = UsnRecordParser.Parse(buffer, 8, buffer.Length).ToList();

        Assert.Equal(2, recs.Count);
        Assert.Equal("a.txt", recs[0].Name);
        Assert.False(recs[0].IsHidden);
        Assert.Equal("b.txt", recs[1].Name);
        Assert.True(recs[1].IsHidden);
    }

    [Fact]
    public void Parse_SkipsNonV2Records()
    {
        // A V3 record (128-bit refs, different layout) must not be misread as V2.
        var buffer = WithCursor(Record(1, 5, 0, 0, "skip.me", major: 3));

        Assert.Empty(UsnRecordParser.Parse(buffer, 8, buffer.Length));
    }
}
