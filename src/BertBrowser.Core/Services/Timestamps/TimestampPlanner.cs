using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Metadata;

namespace BertBrowser.Core.Services.Timestamps;

/// <summary>What the planner needs to know about disk, so it can be tested without one.</summary>
public interface ITimestampProbe
{
    TimestampEntry? Find(string path);

    /// <summary>The picture's own date taken in local time, or null when it has none.</summary>
    DateTime? DateTakenOf(string path);
}

public sealed class FileSystemTimestampProbe : ITimestampProbe
{
    public TimestampEntry? Find(string path)
    {
        try
        {
            FileSystemInfo info = Directory.Exists(path) ? new DirectoryInfo(path) : new FileInfo(path);
            return info.Exists
                ? new TimestampEntry(info.Attributes, info.LastWriteTimeUtc, info.CreationTimeUtc)
                : null;
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
            return null;
        }
    }

    public DateTime? DateTakenOf(string path)
    {
        if (MetadataCodecs.For(path) is not { } codec) return null;

        try
        {
            using var stream = ReadOnlyFile.TryOpen(path);
            if (stream is null) return null;

            return MetadataFields.TryParseDate(codec.Read(stream).Get(MetadataField.DateTaken), out var taken)
                ? taken
                : null;
        }
        catch (MetadataFormatException)
        {
            return null;
        }
        catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
        {
            return null;
        }
    }
}

/// <summary>
/// Works out the dates each selected item should end up with. Pure: every question about disk
/// goes through <see cref="ITimestampProbe"/>.
/// </summary>
public sealed class TimestampPlanner
{
    /// <summary>The earliest date NTFS can store, with a day's margin for time zones.</summary>
    private static readonly DateTime Earliest = new(1601, 1, 2, 0, 0, 0, DateTimeKind.Utc);

    private readonly ITimestampProbe _probe;

    public TimestampPlanner(ITimestampProbe probe) => _probe = probe;

    public TimestampPlanner() : this(new FileSystemTimestampProbe())
    {
    }

    public TimestampPlan Plan(IReadOnlyList<string> paths, TimestampChange change)
    {
        if (!change.Modified && !change.Created) return TimestampPlan.Empty;

        var items = new List<PlannedTimestamp>();
        var rejected = new List<RejectedTimestamp>();

        foreach (var path in paths.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            var name = Path.GetFileName(path) is { Length: > 0 } leaf ? leaf : path;
            void No(TimestampRejection reason, string message) => rejected.Add(new(path, reason, message));

            if (MetadataEditPlanner.IsInsideArchive(path))
            {
                No(TimestampRejection.InsideArchive, $"{name} is inside an archive.");
                continue;
            }

            if (_probe.Find(path) is not { } entry)
            {
                No(TimestampRejection.Missing, $"{name} is no longer there.");
                continue;
            }

            if ((entry.Attributes & FileAttributes.ReparsePoint) != 0)
            {
                No(TimestampRejection.Link, $"{name} is a link, and its date is its target's.");
                continue;
            }

            DateTime? modified, created;
            try
            {
                switch (change.Source)
                {
                    case TimestampSource.Shift:
                        var offset = change.Offset ?? TimeSpan.Zero;
                        modified = entry.ModifiedUtc + offset;
                        created = entry.CreatedUtc + offset;
                        break;

                    case TimestampSource.DateTaken:
                        if (_probe.DateTakenOf(path) is not { } taken)
                        {
                            No(TimestampRejection.NoDateTaken, $"{name} has no date taken.");
                            continue;
                        }
                        modified = created = DateTime.SpecifyKind(taken, DateTimeKind.Local).ToUniversalTime();
                        break;

                    default:
                        if (change.Value is not { } value) return TimestampPlan.Empty;
                        modified = created = DateTime.SpecifyKind(value, DateTimeKind.Local).ToUniversalTime();
                        break;
                }
            }
            catch (ArgumentOutOfRangeException)
            {
                No(TimestampRejection.OutOfRange, $"That is not a date {name} can be given.");
                continue;
            }

            if (!change.Modified) modified = null;
            if (!change.Created) created = null;

            if (modified < Earliest || created < Earliest)
            {
                No(TimestampRejection.OutOfRange, $"That is not a date {name} can be given.");
                continue;
            }

            if ((modified is null || modified == entry.ModifiedUtc) && (created is null || created == entry.CreatedUtc))
            {
                No(TimestampRejection.Unchanged, $"{name} already has that date.");
                continue;
            }

            items.Add(new PlannedTimestamp(
                path, (entry.Attributes & FileAttributes.Directory) != 0, modified, created));
        }

        return new TimestampPlan(items, rejected);
    }
}
