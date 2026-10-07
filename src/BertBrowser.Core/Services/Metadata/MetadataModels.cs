namespace BertBrowser.Core.Services.Metadata;

/// <summary>What one file holds, in canonical values. A field with nothing in it is absent.</summary>
/// <param name="HasLocation">The file says where it was made — GPS coordinates. Not a field,
/// because it is only ever removed, never typed.</param>
public sealed record MetadataDocument(
    MetadataFamily Family,
    IReadOnlyDictionary<MetadataField, string> Values,
    bool HasLocation = false)
{
    public string Get(MetadataField field) => Values.GetValueOrDefault(field, "");

    /// <summary>The embedded cover picture — an album's sleeve — as the image file it is, or null.</summary>
    public byte[]? Picture { get; init; }
}

/// <summary>
/// What to change. A field that is not named is left exactly as it is — which is how a selection
/// of forty files with forty titles keeps them while their album is set.
/// </summary>
/// <param name="Changes">Canonical values from <see cref="MetadataFields.TryNormalize"/>; an empty
/// one clears the field.</param>
public sealed record MetadataEdit(IReadOnlyDictionary<MetadataField, string> Changes)
{
    public static MetadataEdit None { get; } = new(new Dictionary<MetadataField, string>());

    /// <summary>
    /// Take out everything that describes the file rather than being it, before
    /// <see cref="Changes"/> are applied. A picture keeps its orientation and its colour profile:
    /// those are how it is drawn, and without them it turns on its side or changes colour.
    /// </summary>
    public bool RemoveAll { get; init; }

    /// <summary>Take out where a picture was taken, and nothing else.</summary>
    public bool RemoveLocation { get; init; }

    /// <summary>Move the date taken by this much — for a camera whose clock was wrong.</summary>
    public TimeSpan? ShiftDateTaken { get; init; }

    /// <summary>Embed this image file as the cover picture, replacing any there.</summary>
    public byte[]? SetPicture { get; init; }

    /// <summary>Take the cover picture out.</summary>
    public bool RemovePicture { get; init; }

    public bool IsEmpty =>
        Changes.Count == 0 && !RemoveAll && !RemoveLocation && ShiftDateTaken is null &&
        SetPicture is null && !RemovePicture;

    /// <summary>The part of this edit a file <paramref name="codec"/> handles can take.</summary>
    public MetadataEdit For(IMetadataCodec codec) => this with
    {
        Changes = Changes.Where(c => codec.Fields.Contains(c.Key)).ToDictionary(c => c.Key, c => c.Value),
        RemoveLocation = RemoveLocation && codec.Family == MetadataFamily.Image,
        ShiftDateTaken = codec.Fields.Contains(MetadataField.DateTaken) ? ShiftDateTaken : null,
        SetPicture = codec.HoldsPicture ? SetPicture : null,
        RemovePicture = RemovePicture && codec.HoldsPicture,
    };
}

/// <summary>The file is not what its name says, or is damaged past reading.</summary>
public sealed class MetadataFormatException(string message) : Exception(message);

public enum MetadataRejection
{
    Missing,
    IsFolder,
    /// <summary>An entry inside an archive: there is no file on disk to rewrite.</summary>
    InsideArchive,
    /// <summary>A cloud file whose bytes are not here; reading it would download it.</summary>
    CloudPlaceholder,
    /// <summary>A link is the entry it is, not the file it points at.</summary>
    Link,
    ReadOnly,
    /// <summary>No codec writes this type.</summary>
    Unsupported,
    /// <summary>None of the fields being changed exist for this kind of file.</summary>
    NothingApplies,
}

public sealed record RejectedMetadataEdit(string Path, MetadataRejection Reason, string Message);

/// <summary>One file and the part of the edit it can take.</summary>
public sealed record PlannedMetadataEdit(string Path, MetadataEdit Edit, long Length)
{
    /// <summary>Written by Windows' property handler rather than a codec — the one path with no
    /// payload check. See <see cref="IPropertyFallback"/>.</summary>
    public bool ViaWindows { get; init; }
}

/// <summary>
/// What editing a selection would amount to. <see cref="RewriteBytes"/> is every planned file
/// whole: a tag cannot be changed without writing the file out again beside itself.
/// </summary>
public sealed record MetadataEditPlan(
    IReadOnlyList<PlannedMetadataEdit> Files,
    IReadOnlyList<RejectedMetadataEdit> Rejected)
{
    public bool HasWork => Files.Count > 0;

    public long RewriteBytes => Files.Sum(f => f.Length);

    public static MetadataEditPlan Empty { get; } = new([], []);
}

/// <summary>
/// A file with its other version held beside it: the original while an edit is in effect, the
/// edited one while it is undone.
/// </summary>
/// <param name="InPlace">What is at <paramref name="Path"/> now. A step refuses a file that no
/// longer matches, since swapping it out would undo somebody else's change.</param>
public sealed record HeldVersion(string Path, string HeldPath, EntryStamp InPlace);

public sealed record FailedMetadataEdit(string Path, string Message, bool AccessDenied = false);

public sealed record MetadataEditOutcome(
    IReadOnlyList<HeldVersion> Edited,
    IReadOnlyList<FailedMetadataEdit> Failed,
    bool Cancelled)
{
    public bool CanUndo => Edited.Count > 0;

    /// <summary>Files the edit was offered to and which already were as it asked: not changed,
    /// not failed, and not in the history.</summary>
    public IReadOnlyList<string> Unchanged { get; init; } = [];

    public static MetadataEditOutcome Empty { get; } = new([], [], false);
}

/// <summary>What an undo or a redo moved, and what it could not.</summary>
public sealed record MetadataSwapResult(
    IReadOnlyList<HeldVersion> Swapped,
    IReadOnlyList<FailedMetadataEdit> Failed);
