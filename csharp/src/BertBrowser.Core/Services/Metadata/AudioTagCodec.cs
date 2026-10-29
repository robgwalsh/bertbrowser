using System.Globalization;
using ATL;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>
/// Audio and video tags, read and written by ATL. <b>The only class that references it</b>, which
/// is what keeps the dependency replaceable — the same arrangement <c>ArchiveReader</c> has with
/// SharpCompress.
/// </summary>
/// <remarks>
/// The library edits a file where it stands, so <see cref="Write"/> copies the source into the
/// destination first and lets it edit the copy. The original is never opened for writing by
/// anything; whether the copy is fit to replace it is the executor's decision, made on
/// <see cref="PayloadDigest"/>, which does not ask the library anything at all.
/// </remarks>
internal sealed class AudioTagCodec(string extension) : IMetadataCodec
{
    static AudioTagCodec()
    {
        // Process-wide switches, set once. A stream has no file name to fall back on anyway, but
        // an invented title must never be read back as though the file held it.
        Settings.UseFileNameWhenNoTitle = false;
        Settings.OutputStacktracesToConsole = false;
    }

    private static readonly HashSet<MetadataField> Everything =
    [
        MetadataField.Title, MetadataField.Artist, MetadataField.AlbumArtist, MetadataField.Album,
        MetadataField.TrackNumber, MetadataField.TrackTotal, MetadataField.DiscNumber, MetadataField.DiscTotal,
        MetadataField.Year, MetadataField.Genre, MetadataField.Composer, MetadataField.Comment,
        MetadataField.Copyright, MetadataField.Publisher, MetadataField.Conductor, MetadataField.Bpm,
        MetadataField.Lyrics,
    ];

    /// <summary>
    /// What each container has no place for. Found by writing every field to a real file of each
    /// kind and reading it back, and pinned by the test that still does: a field offered here
    /// that a format silently drops would fail the executor's read-back on every file of that kind.
    /// </summary>
    private static readonly Dictionary<string, MetadataField[]> Missing = new()
    {
        [".wma"] = [MetadataField.Publisher],
    };

    public MetadataFamily Family => MetadataFamily.Audio;

    public IReadOnlySet<MetadataField> Fields { get; } =
        Missing.TryGetValue(extension, out var missing)
            ? Everything.Except(missing).ToHashSet()
            : Everything;

    public bool HoldsPicture => !NoPicture.Contains(extension);

    private static readonly HashSet<string> NoPicture = [];

    public MetadataDocument Read(Stream source)
    {
        // Ours first: it is what decides whether this is the format its name claims.
        AudioPayload.Digest(extension, source);

        source.Position = 0;
        var track = Open(source);
        var values = new Dictionary<MetadataField, string>();

        void Text(MetadataField field, string? value)
        {
            if (Fields.Contains(field) && !string.IsNullOrWhiteSpace(value))
                values[field] = value.Trim().Replace("\r\n", "\n").Replace('\r', '\n');
        }

        void Number(MetadataField field, int? value)
        {
            if (Fields.Contains(field) && value is > 0)
                values[field] = value.Value.ToString(CultureInfo.InvariantCulture);
        }

        Text(MetadataField.Title, track.Title);
        Text(MetadataField.Artist, track.Artist);
        Text(MetadataField.AlbumArtist, track.AlbumArtist);
        Text(MetadataField.Album, track.Album);
        Text(MetadataField.Genre, track.Genre);
        Text(MetadataField.Composer, track.Composer);
        Text(MetadataField.Conductor, track.Conductor);
        Text(MetadataField.Publisher, track.Publisher);
        Text(MetadataField.Comment, track.Comment);
        Text(MetadataField.Copyright, track.Copyright);
        Text(MetadataField.Lyrics, track.Lyrics.FirstOrDefault(l => !string.IsNullOrWhiteSpace(l.UnsynchronizedLyrics))?.UnsynchronizedLyrics);
        Number(MetadataField.TrackNumber, track.TrackNumber);
        Number(MetadataField.TrackTotal, track.TrackTotal);
        Number(MetadataField.DiscNumber, track.DiscNumber);
        Number(MetadataField.DiscTotal, track.DiscTotal);
        Number(MetadataField.Year, track.Year);
        Number(MetadataField.Bpm, track.BPM is { } bpm ? (int)Math.Round(bpm) : null);

        // The front cover when one is marked as such, else whatever picture comes first.
        var picture = HoldsPicture
            ? (track.EmbeddedPictures.FirstOrDefault(p => p.PicType == PictureInfo.PIC_TYPE.Front)
               ?? track.EmbeddedPictures.FirstOrDefault())?.PictureData
            : null;

        return new MetadataDocument(Family, values) { Picture = picture is { Length: > 0 } ? picture : null };
    }

    public void Write(Stream source, Stream destination, MetadataEdit edit, CancellationToken ct)
    {
        AudioPayload.Digest(extension, source);

        source.Position = 0;
        var buffer = new byte[128 * 1024];
        int read;
        while ((read = source.Read(buffer, 0, buffer.Length)) > 0)
        {
            ct.ThrowIfCancellationRequested();
            destination.Write(buffer, 0, read);
        }

        destination.Position = 0;
        var track = Open(destination);

        if (edit.RemoveAll)
        {
            // Every tag the file carries, of every kind — an MP3 can have three at once.
            if (!track.Remove(ATL.AudioData.MetaDataIOFactory.TagType.ANY))
                throw new MetadataFormatException("The tags could not be removed, so the file was left as it is.");

            destination.Flush();
            if (edit.Changes.Count == 0 && edit.SetPicture is null) return;

            destination.Position = 0;
            track = Open(destination);
        }

        foreach (var (field, value) in edit.Changes)
        {
            int? number = int.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var parsed)
                ? parsed
                : 0;

            switch (field)
            {
                case MetadataField.Title: track.Title = value; break;
                case MetadataField.Artist: track.Artist = value; break;
                case MetadataField.AlbumArtist: track.AlbumArtist = value; break;
                case MetadataField.Album: track.Album = value; break;
                case MetadataField.Genre: track.Genre = value; break;
                case MetadataField.Composer: track.Composer = value; break;
                case MetadataField.Conductor: track.Conductor = value; break;
                case MetadataField.Publisher: track.Publisher = value; break;
                case MetadataField.Comment: track.Comment = value; break;
                case MetadataField.Copyright: track.Copyright = value; break;
                case MetadataField.TrackNumber: track.TrackNumber = number; break;
                case MetadataField.TrackTotal: track.TrackTotal = number; break;
                case MetadataField.DiscNumber: track.DiscNumber = number; break;
                case MetadataField.DiscTotal: track.DiscTotal = number; break;
                case MetadataField.Year: track.Year = number; break;
                case MetadataField.Bpm: track.BPM = number; break;
                case MetadataField.Lyrics:
                    track.Lyrics.Clear();
                    if (value.Length > 0) track.Lyrics.Add(new LyricsInfo { UnsynchronizedLyrics = value });
                    break;
            }
        }

        if (edit.RemovePicture || edit.SetPicture is not null)
        {
            track.EmbeddedPictures.Clear();
            if (edit.SetPicture is { } picture)
                track.EmbeddedPictures.Add(PictureInfo.fromBinaryData(picture, PictureInfo.PIC_TYPE.Front));
        }

        // Every one of these formats stores a total as the second half of "3/12". With no first
        // half there is nowhere to put it, and it would be dropped without a word.
        if (track.TrackTotal is > 0 && track.TrackNumber is not > 0)
            throw new MetadataFormatException("A number of tracks needs a track number to go with it.");
        if (track.DiscTotal is > 0 && track.DiscNumber is not > 0)
            throw new MetadataFormatException("A number of discs needs a disc number to go with it.");

        ct.ThrowIfCancellationRequested();
        if (!track.Save())
            throw new MetadataFormatException("The tags could not be written, so the file was left as it is.");

        destination.Flush();
    }

    public byte[] PayloadDigest(Stream source) => AudioPayload.Digest(extension, source);

    private Track Open(Stream stream)
    {
        try
        {
            return new Track(stream, extension);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            // Whatever a third-party parser throws at a damaged file, it is one answer to us.
            throw new MetadataFormatException("This is not a readable audio file, so it was left as it is.");
        }
    }
}
