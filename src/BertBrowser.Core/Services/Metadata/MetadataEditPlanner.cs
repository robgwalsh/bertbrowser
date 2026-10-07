using BertBrowser.Core.Services.Archives;

namespace BertBrowser.Core.Services.Metadata;

/// <summary>What the planner needs to know about disk, so it can be tested without one.</summary>
public interface IMetadataProbe
{
    /// <summary>The entry's attributes, or null when nothing is there.</summary>
    FileAttributes? AttributesOf(string path);

    long LengthOf(string path);
}

public sealed class FileSystemMetadataProbe : IMetadataProbe
{
    public FileAttributes? AttributesOf(string path)
    {
        try
        {
            return File.GetAttributes(path);
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
            return null;
        }
    }

    public long LengthOf(string path)
    {
        try
        {
            return new FileInfo(path).Length;
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
            return 0;
        }
    }
}

/// <summary>
/// Decides which files of a selection an edit can be written to, and says why not for the rest.
/// Pure: every question about disk goes through <see cref="IMetadataProbe"/>.
/// </summary>
public sealed class MetadataEditPlanner
{
    private readonly IMetadataProbe _probe;

    public MetadataEditPlanner(IMetadataProbe probe) => _probe = probe;

    public MetadataEditPlanner() : this(new FileSystemMetadataProbe())
    {
    }

    /// <param name="viaWindows">For files no codec here writes: the fields Windows' own property
    /// handler said it would take, by path (see <see cref="IPropertyFallback"/>). Taken rather
    /// than asked for, so planning stays pure — asking is opening the file.</param>
    public MetadataEditPlan Plan(
        IReadOnlyList<string> paths,
        MetadataEdit edit,
        IReadOnlyDictionary<string, IReadOnlySet<MetadataField>>? viaWindows = null)
    {
        var files = new List<PlannedMetadataEdit>();
        var rejected = new List<RejectedMetadataEdit>();

        foreach (var path in paths.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            // A codec always wins: the fallback is for what nothing here can check.
            IReadOnlySet<MetadataField>? offered = null;
            var windows = !MetadataCodecs.Handles(path) && viaWindows?.TryGetValue(path, out offered) == true;

            if (Refusal(path, _probe.AttributesOf(path), windows) is { } refusal)
            {
                rejected.Add(refusal);
                continue;
            }

            if (windows)
            {
                // Fields only. A removal is a promise about the whole file, and a handler that
                // cannot be read behind is not something to make one through.
                var changes = edit.Changes
                    .Where(c => offered!.Contains(c.Key))
                    .ToDictionary(c => c.Key, c => c.Value);
                if (changes.Count == 0)
                {
                    rejected.Add(new RejectedMetadataEdit(path, MetadataRejection.NothingApplies,
                        $"Windows can change none of this in {Path.GetFileName(path)}."));
                    continue;
                }

                files.Add(new PlannedMetadataEdit(path, new MetadataEdit(changes), _probe.LengthOf(path)) { ViaWindows = true });
                continue;
            }

            var codec = MetadataCodecs.For(path)!;
            var applicable = edit.For(codec);
            if (applicable.IsEmpty)
            {
                rejected.Add(new RejectedMetadataEdit(path, MetadataRejection.NothingApplies,
                    $"None of these fields exist for {Describe(codec.Family)}."));
                continue;
            }

            files.Add(new PlannedMetadataEdit(path, applicable, _probe.LengthOf(path)));
        }

        return new MetadataEditPlan(files, rejected);
    }

    /// <summary>
    /// Why this file cannot be edited, or null when it can. The executor asks again against live
    /// disk before it writes, because a plan describes the moment it was made.
    /// </summary>
    /// <param name="viaWindows">The file is to be written by Windows' property handler rather
    /// than a codec, so having no codec is not a reason. Every other refusal still is.</param>
    public static RejectedMetadataEdit? Refusal(string path, FileAttributes? attributes, bool viaWindows = false)
    {
        RejectedMetadataEdit No(MetadataRejection reason, string message) => new(path, reason, message);
        var name = Path.GetFileName(path);

        // Asked first, and of the path alone: an entry inside an archive has no attributes to
        // probe, and looks like an ordinary missing file to anything that tries.
        if (IsInsideArchive(path))
            return No(MetadataRejection.InsideArchive, $"{name} is inside an archive. Extract it to edit it.");

        if (attributes is not { } found)
            return No(MetadataRejection.Missing, $"{name} is no longer there.");

        if ((found & FileAttributes.Directory) != 0)
            return No(MetadataRejection.IsFolder, $"{name} is a folder.");

        if ((found & FileAttributes.ReparsePoint) != 0)
            return No(MetadataRejection.Link, $"{name} is a link, and a link is not edited through.");

        if ((found & ReadOnlyFile.Placeholder) != 0)
            return No(MetadataRejection.CloudPlaceholder,
                $"{name} is not downloaded to this PC, and editing it would download it.");

        if (!viaWindows && !MetadataCodecs.Handles(path))
            return No(MetadataRejection.Unsupported,
                Path.GetExtension(path).TrimStart('.') is { Length: > 0 } extension
                    ? $"{extension.ToUpperInvariant()} files cannot be edited."
                    : "This kind of file cannot be edited.");

        if ((found & FileAttributes.ReadOnly) != 0)
            return No(MetadataRejection.ReadOnly, $"{name} is read-only.");

        return null;
    }

    /// <summary>
    /// Whether a path names an entry inside an archive. Asked of its folder, never of its own
    /// name: an EPUB or a .docx-shaped zip is an archive too, and standing on one is not being in it.
    /// </summary>
    public static bool IsInsideArchive(string path) => ArchivePath.LooksVirtual(Path.GetDirectoryName(path));

    private static string Describe(MetadataFamily family) => family switch
    {
        MetadataFamily.Image => "a picture",
        MetadataFamily.Audio => "a song or video",
        MetadataFamily.Document => "a document",
        _ => "this kind of file",
    };
}
