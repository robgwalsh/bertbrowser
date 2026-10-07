using System.Buffers.Binary;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>Which directory of an EXIF block a tag lives in.</summary>
internal enum ExifDirectory
{
    /// <summary>IFD0: the picture's own description, and the Windows <c>XP*</c> tags.</summary>
    Image,
    /// <summary>The Exif sub-directory: how and when it was taken.</summary>
    Photo,
}

/// <summary>
/// An EXIF block — a small TIFF file carried inside a picture — that can be edited without moving
/// anything already in it.
/// </summary>
/// <remarks>
/// <para>
/// <b>Nothing that exists is relocated.</b> A camera's MakerNote is an opaque blob full of offsets
/// measured from the start of this block, and nothing outside the manufacturer knows which bytes
/// they are. A writer that re-serialises the block tidily moves the blob and silently breaks every
/// one of them. So a changed value is written over its own old bytes when it fits and appended to
/// the end when it does not; a directory that grew is appended whole and the one pointer to it is
/// redirected. The cost is a few dead bytes per edit, and that is the whole cost.
/// </para>
/// <para>
/// <b>Dead bytes are zeroed</b>, because a removed value that is still sitting in the file has not
/// been removed — which matters the day the value is where a photograph was taken. The exception is
/// a range something else also points into (some writers share one copy of a repeated value): that
/// is left alone and never reused, since the other owner still needs it.
/// </para>
/// <para>
/// Structure this class has to write into is parsed strictly and a damaged one is refused. A value
/// it merely carries is parsed leniently: an entry pointing off the end of the block is kept
/// byte-for-byte and reads as absent.
/// </para>
/// </remarks>
internal sealed class ExifBlock
{
    /// <summary>The most a JPEG APP1 segment can carry after its <c>Exif\0\0</c> label.</summary>
    public const int MaxLength = 65_527;

    public const ushort TypeByte = 1;
    public const ushort TypeAscii = 2;
    public const ushort TypeShort = 3;
    public const ushort TypeLong = 4;
    public const ushort TypeRational = 5;
    public const ushort TypeUndefined = 7;

    private const ushort ExifPointer = 0x8769;
    private const ushort GpsPointer = 0x8825;
    private const ushort InteropPointer = 0xA005;
    private const ushort ThumbnailOffset = 0x0201;
    private const ushort ThumbnailLength = 0x0202;

    private readonly byte[] _original;
    private readonly bool _little;
    private readonly Directory _image;
    private Directory? _photo;
    private Directory? _gps;

    /// <summary>Every byte range a parsed entry or directory occupies, so a range two things
    /// share can be told from one that is safe to overwrite.</summary>
    private readonly List<(int Offset, int Length, object Owner)> _claims = [];

    private readonly List<(int Offset, int Length, object Owner)> _abandoned = [];

    private ExifBlock(byte[] original, bool little, Directory image)
    {
        _original = original;
        _little = little;
        _image = image;
    }

    /// <summary>A block with nothing in it, for a picture that has none.</summary>
    public static ExifBlock Empty()
    {
        byte[] header = [(byte)'I', (byte)'I', 42, 0, 0, 0, 0, 0];
        return new ExifBlock(header, little: true, new Directory { Offset = -1 });
    }

    /// <exception cref="MetadataFormatException">Not a TIFF structure, or damaged where it
    /// would have to be written.</exception>
    public static ExifBlock Parse(ReadOnlySpan<byte> tiff)
    {
        if (tiff.Length < 8) throw Damaged();

        bool little;
        if (tiff[0] == 'I' && tiff[1] == 'I') little = true;
        else if (tiff[0] == 'M' && tiff[1] == 'M') little = false;
        else throw Damaged();

        var data = tiff.ToArray();
        if (U16(data, 2, little) != 42) throw Damaged();

        var block = new ExifBlock(data, little, new Directory());
        var seen = new HashSet<int>();

        var image = block.ReadDirectory(checked((int)Math.Min(U32(data, 4, little), int.MaxValue)), seen)
                    ?? throw Damaged();
        block._image.CopyFrom(image);
        block.Reclaim(image, block._image);

        if (block.PointerOf(block._image, ExifPointer) is { } photoAt)
            block._photo = block.ReadDirectory(photoAt, seen) ?? throw Damaged();

        // The GPS directory is never written to, only removed whole; the rest are walked so their
        // bytes are known to be in use.
        if (block.PointerOf(block._image, GpsPointer) is { } gpsAt) block._gps = block.ReadDirectory(gpsAt, seen);
        if (block._photo is { } photo && block.PointerOf(photo, InteropPointer) is { } interopAt)
            block.ReadDirectory(interopAt, seen);

        var next = block._image.Next;
        while (next != 0 && block.ReadDirectory(checked((int)Math.Min(next, int.MaxValue)), seen) is { } thumbnail)
        {
            if (block.PointerOf(thumbnail, ThumbnailOffset) is { } at &&
                block.PointerOf(thumbnail, ThumbnailLength) is { } length &&
                at >= 0 && length > 0 && (long)at + length <= data.Length)
                block._claims.Add((at, length, thumbnail));

            next = thumbnail.Next;
        }

        return block;
    }

    /// <summary>The raw bytes of a tag's value, or null when it is absent or unreadable.</summary>
    public byte[]? Get(ExifDirectory directory, ushort tag)
    {
        var entry = DirectoryOf(directory)?.Find(tag);
        if (entry is null) return null;
        if (entry.Replacement is { } replaced) return replaced;
        if (entry.Opaque) return null;

        return entry.DataOffset >= 0
            ? _original.AsSpan(entry.DataOffset, entry.DataLength).ToArray()
            : entry.Inline.AsSpan(0, entry.DataLength).ToArray();
    }

    public ushort? TypeOf(ExifDirectory directory, ushort tag) => DirectoryOf(directory)?.Find(tag)?.Type;

    public void Set(ExifDirectory directory, ushort tag, ushort type, byte[] value)
    {
        var size = SizeOf(type);
        if (size == 0 || value.Length == 0 || value.Length % size != 0)
            throw new ArgumentException("Not a value of that type.", nameof(value));

        var target = DirectoryOf(directory) ?? (_photo = new Directory { Offset = -1 });

        var entry = target.Find(tag);
        if (entry is null)
        {
            entry = new Entry { Tag = tag, DataOffset = -1 };
            target.Entries.Add(entry);
        }

        entry.Type = type;
        entry.Count = (uint)(value.Length / size);
        entry.Replacement = value;
        entry.Opaque = false;
        target.Dirty = true;
    }

    public void Remove(ExifDirectory directory, ushort tag)
    {
        var target = DirectoryOf(directory);
        if (target?.Find(tag) is not { } entry) return;

        target.Entries.Remove(entry);
        target.Dirty = true;
        if (entry.DataOffset >= 0) _abandoned.Add((entry.DataOffset, entry.DataLength, entry));
    }

    /// <summary>
    /// Whether the block says where the picture was taken. True for a GPS pointer this could not
    /// follow, too: an unreadable location is still one, and "none here" must not be the answer.
    /// </summary>
    public bool HasLocation => _gps is not null || _image.Find(GpsPointer) is not null;

    /// <summary>
    /// Takes the GPS directory out: the pointer to it, the table, and every value it held, zeroed.
    /// </summary>
    /// <exception cref="MetadataFormatException">The directory is damaged, so its bytes cannot
    /// be found to erase — and unlinking it while leaving them would only look like removal.</exception>
    /// <summary>
    /// Takes the whole Exif sub-directory out — how and when the picture was taken, the maker's
    /// note, serial numbers — zeroing every value it held. For a format where the block cannot
    /// simply be dropped, because the picture itself lives in it.
    /// </summary>
    public void RemovePhotoDirectory()
    {
        if (_photo is not { } photo)
        {
            Remove(ExifDirectory.Image, ExifPointer);
            return;
        }

        foreach (var entry in photo.Entries)
            if (entry.DataOffset >= 0) _abandoned.Add((entry.DataOffset, entry.DataLength, entry));

        if (photo.Offset >= 0) _abandoned.Add((photo.Offset, photo.TableLength, photo));
        _photo = null;
        Remove(ExifDirectory.Image, ExifPointer);
    }

    public void RemoveLocation()
    {
        if (!HasLocation) return;

        if (_gps is not { } gps)
            throw new MetadataFormatException(
                "This picture's location is stored in a damaged block that cannot be safely erased.");

        foreach (var entry in gps.Entries)
        {
            if (entry.Opaque)
                throw new MetadataFormatException(
                    "This picture's location is stored in a damaged block that cannot be safely erased.");
            if (entry.DataOffset >= 0) _abandoned.Add((entry.DataOffset, entry.DataLength, entry));
        }

        _abandoned.Add((gps.Offset, gps.TableLength, gps));
        _gps = null;
        Remove(ExifDirectory.Image, GpsPointer);
    }

    public ushort ReadU16(ReadOnlySpan<byte> value) =>
        _little ? BinaryPrimitives.ReadUInt16LittleEndian(value) : BinaryPrimitives.ReadUInt16BigEndian(value);

    public byte[] WriteU16(ushort value)
    {
        var bytes = new byte[2];
        if (_little) BinaryPrimitives.WriteUInt16LittleEndian(bytes, value);
        else BinaryPrimitives.WriteUInt16BigEndian(bytes, value);
        return bytes;
    }

    public bool IsLittleEndian => _little;

    public uint ReadU32(ReadOnlySpan<byte> value) =>
        _little ? BinaryPrimitives.ReadUInt32LittleEndian(value) : BinaryPrimitives.ReadUInt32BigEndian(value);

    /// <summary>
    /// The block with every change applied.
    /// </summary>
    /// <exception cref="MetadataFormatException">The result would not fit in one segment.</exception>
    /// <param name="maxLength">The most the result may be. A JPEG segment's limit by default; a
    /// TIFF file, which is one of these blocks from end to end, has none of its own.</param>
    public byte[] ToArray(int maxLength = MaxLength)
    {
        var buffer = new List<byte>(_original);

        foreach (var (offset, length, owner) in _abandoned) Zero(buffer, offset, length, owner);

        if (_photo is { } photo)
        {
            Commit(buffer, photo, at =>
            {
                Set(ExifDirectory.Image, ExifPointer, TypeLong, WriteU32((uint)at));
            });
        }

        Commit(buffer, _image, at =>
        {
            var pointer = WriteU32((uint)at);
            for (var i = 0; i < pointer.Length; i++) buffer[4 + i] = pointer[i];
        });

        if (buffer.Count > maxLength)
            throw new MetadataFormatException("There is no room left in this picture's EXIF block.");

        return [.. buffer];
    }

    private void Commit(List<byte> buffer, Directory directory, Action<int> redirect)
    {
        if (!directory.Dirty && directory.Offset >= 0) return;

        foreach (var entry in directory.Entries)
        {
            if (entry.Replacement is not { } value) continue;

            if (value.Length <= 4)
            {
                if (entry.DataOffset >= 0) Zero(buffer, entry.DataOffset, entry.DataLength, entry);
                entry.Inline = new byte[4];
                value.CopyTo(entry.Inline, 0);
                entry.DataOffset = -1;
            }
            else if (entry.DataOffset >= 0 && entry.DataLength >= value.Length &&
                     !Shared(entry.DataOffset, entry.DataLength, entry))
            {
                Zero(buffer, entry.DataOffset, entry.DataLength, entry);
                for (var i = 0; i < value.Length; i++) buffer[entry.DataOffset + i] = value[i];
                entry.Inline = WriteU32((uint)entry.DataOffset);
            }
            else
            {
                if (entry.DataOffset >= 0) Zero(buffer, entry.DataOffset, entry.DataLength, entry);
                entry.DataOffset = Append(buffer, value);
                entry.Inline = WriteU32((uint)entry.DataOffset);
            }

            entry.DataLength = value.Length;
            entry.Replacement = null;
        }

        var entries = directory.Entries.OrderBy(e => e.Tag).ToList();
        var table = new byte[2 + 12 * entries.Count + 4];
        WriteU16((ushort)entries.Count).CopyTo(table, 0);
        for (var i = 0; i < entries.Count; i++)
        {
            var at = 2 + 12 * i;
            WriteU16(entries[i].Tag).CopyTo(table, at);
            WriteU16(entries[i].Type).CopyTo(table, at + 2);
            WriteU32(entries[i].Count).CopyTo(table, at + 4);
            entries[i].Inline.CopyTo(table, at + 8);
        }
        WriteU32(directory.Next).CopyTo(table, table.Length - 4);

        var room = directory.Offset >= 0 ? directory.TableLength : 0;
        if (directory.Offset >= 0 && table.Length <= room && !Shared(directory.Offset, room, directory))
        {
            Zero(buffer, directory.Offset, room, directory);
            for (var i = 0; i < table.Length; i++) buffer[directory.Offset + i] = table[i];
        }
        else
        {
            if (directory.Offset >= 0) Zero(buffer, directory.Offset, room, directory);
            directory.Offset = Append(buffer, table);
            redirect(directory.Offset);
        }

        directory.TableLength = table.Length;
        directory.Dirty = false;
    }

    /// <summary>Adds bytes at the end, on an even offset as TIFF asks, and says where.</summary>
    private static int Append(List<byte> buffer, byte[] value)
    {
        if (buffer.Count % 2 != 0) buffer.Add(0);
        var at = buffer.Count;
        buffer.AddRange(value);
        return at;
    }

    private void Zero(List<byte> buffer, int offset, int length, object owner)
    {
        if (Shared(offset, length, owner)) return;
        for (var i = 0; i < length; i++) buffer[offset + i] = 0;
    }

    private bool Shared(int offset, int length, object owner)
    {
        foreach (var claim in _claims)
        {
            if (ReferenceEquals(claim.Owner, owner)) continue;
            if (claim.Offset < offset + length && offset < claim.Offset + claim.Length) return true;
        }
        return false;
    }

    private Directory? DirectoryOf(ExifDirectory directory) =>
        directory == ExifDirectory.Image ? _image : _photo;

    private int? PointerOf(Directory directory, ushort tag)
    {
        var entry = directory.Find(tag);
        if (entry is null || entry.Opaque || entry.DataOffset >= 0 || entry.Count != 1) return null;

        return entry.Type switch
        {
            TypeLong => checked((int)Math.Min(U32(entry.Inline, 0, _little), int.MaxValue)),
            TypeShort => U16(entry.Inline, 0, _little),
            _ => null,
        };
    }

    /// <summary>Reads one directory, or returns null when it is not there to be read.</summary>
    private Directory? ReadDirectory(int offset, HashSet<int> seen)
    {
        // A directory already walked is a loop, which a hostile file can make as long as it likes.
        if (offset < 8 || !seen.Add(offset)) return null;
        if ((long)offset + 2 > _original.Length) return null;

        var count = U16(_original, offset, _little);
        var length = 2 + 12 * count + 4;
        if ((long)offset + length > _original.Length) return null;

        var directory = new Directory { Offset = offset, TableLength = length };
        for (var i = 0; i < count; i++)
        {
            var at = offset + 2 + 12 * i;
            var entry = new Entry
            {
                Tag = U16(_original, at, _little),
                Type = U16(_original, at + 2, _little),
                Count = U32(_original, at + 4, _little),
                Inline = _original.AsSpan(at + 8, 4).ToArray(),
                DataOffset = -1,
            };

            var size = (long)SizeOf(entry.Type) * entry.Count;
            if (size == 0)
            {
                entry.Opaque = true;
            }
            else if (size <= 4)
            {
                entry.DataLength = (int)size;
            }
            else
            {
                var where = U32(entry.Inline, 0, _little);
                if (where + size > _original.Length)
                {
                    entry.Opaque = true;
                }
                else
                {
                    entry.DataOffset = (int)where;
                    entry.DataLength = (int)size;
                    _claims.Add((entry.DataOffset, entry.DataLength, entry));
                }
            }

            directory.Entries.Add(entry);
        }

        directory.Next = U32(_original, offset + length - 4, _little);
        _claims.Add((offset, length, directory));
        return directory;
    }

    /// <summary>Moves the claims a throwaway directory made onto the one that is kept.</summary>
    private void Reclaim(Directory from, Directory to)
    {
        for (var i = 0; i < _claims.Count; i++)
            if (ReferenceEquals(_claims[i].Owner, from))
                _claims[i] = (_claims[i].Offset, _claims[i].Length, to);
    }

    private static int SizeOf(ushort type) => type switch
    {
        1 or 2 or 6 or 7 => 1,
        3 or 8 => 2,
        4 or 9 or 11 => 4,
        5 or 10 or 12 => 8,
        _ => 0,
    };

    public byte[] WriteU32(uint value)
    {
        var bytes = new byte[4];
        if (_little) BinaryPrimitives.WriteUInt32LittleEndian(bytes, value);
        else BinaryPrimitives.WriteUInt32BigEndian(bytes, value);
        return bytes;
    }

    private static ushort U16(byte[] data, int at, bool little) =>
        little
            ? BinaryPrimitives.ReadUInt16LittleEndian(data.AsSpan(at, 2))
            : BinaryPrimitives.ReadUInt16BigEndian(data.AsSpan(at, 2));

    private static uint U32(byte[] data, int at, bool little) =>
        little
            ? BinaryPrimitives.ReadUInt32LittleEndian(data.AsSpan(at, 4))
            : BinaryPrimitives.ReadUInt32BigEndian(data.AsSpan(at, 4));

    private static MetadataFormatException Damaged() =>
        new("This picture's EXIF block is damaged, so it was left as it is.");

    private sealed class Directory
    {
        /// <summary>Where the table is, or -1 for one that has not been written yet.</summary>
        public int Offset;
        public int TableLength;
        public uint Next;
        public bool Dirty;
        public List<Entry> Entries = [];

        public Entry? Find(ushort tag) => Entries.FirstOrDefault(e => e.Tag == tag);

        public void CopyFrom(Directory other)
        {
            Offset = other.Offset;
            TableLength = other.TableLength;
            Next = other.Next;
            Entries = other.Entries;
        }
    }

    private sealed class Entry
    {
        public ushort Tag;
        public ushort Type;
        public uint Count;
        /// <summary>The four value bytes as stored: the value itself, or the offset of it.</summary>
        public byte[] Inline = new byte[4];
        /// <summary>Where the value is when it does not fit inline, else -1.</summary>
        public int DataOffset;
        public int DataLength;
        public byte[]? Replacement;
        /// <summary>Carried byte-for-byte and never interpreted: an unknown type, or a value that
        /// points off the end of the block.</summary>
        public bool Opaque;
    }
}
