using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>The ceilings one expansion works within.</summary>
public static class TransferMergeLimits
{
    /// <summary>
    /// How many items one folder may expand into.
    /// </summary>
    /// <remarks>
    /// Every one of them becomes a row in the progress window, a <c>BeginItem</c> report, and — if it
    /// clashes — a row of radio buttons in a dialog. Ten thousand questions is not a question anyone
    /// can answer, and past some size the wholesale question is genuinely the more useful one.
    /// </remarks>
    public const int MaxExpandedTransfers = 5_000;

    /// <summary>
    /// How many directories one expansion may open. Bounds the <em>time</em> as well as the result,
    /// so a pathological overlap cannot spend a minute enumerating before discovering it is too big
    /// — with the user waiting on a dialog that has not appeared yet.
    /// </summary>
    public const int MaxDirectoriesVisited = 2_000;
}

/// <summary>
/// Turns a folder that clashes with a folder into the handful of items inside it that actually
/// clash.
/// </summary>
/// <remarks>
/// <para>
/// <b>Why this is a separate step rather than part of the planner.</b> <c>DropPipeline.IsAllowed</c>
/// builds a plan on every drag-over to decide what the cursor should say. A planner that opened
/// directories would hit the disk on hover, over and over, for a question that only needs a yes or
/// no. So the planner keeps taking the narrower <see cref="ITransferProbe"/> — which cannot
/// enumerate — and this runs once, at the drop, off the UI thread.
/// </para>
/// <para>
/// <b>What it emits.</b> Still one <see cref="TransferPlan"/>, with the same destination directory;
/// the items are simply spread across the merged subtree. That works against an unmodified
/// <see cref="TransferExecutor"/> because a <see cref="PlannedTransfer"/>'s destination is a full
/// path, and — the load-bearing part — <b>every emitted destination's parent already exists</b>,
/// because the walk descends only where <em>both</em> sides have a real directory. Nothing here ever
/// has to invent a folder nobody asked for, which is the same property <c>SyncPlanner</c> keeps by
/// acting on a folder whole or not at all.
/// </para>
/// <para>
/// <b>A link is always a leaf.</b> Neither side may be a junction or symbolic link for the walk to
/// descend. The source rule is the ordinary one — a link cannot be reproduced by copying what is
/// inside it. The destination rule is the one that matters more: a junction at the destination
/// pointing back into the source would merge a folder into itself, one file at a time, by a path
/// <c>TransferExecutor.Revalidate</c> never inspects — it only ever compares a source against
/// <see cref="TransferPlan.DestinationDirectory"/>, and an expanded source is always deeper than
/// that.
/// </para>
/// <para>
/// It also fills in <see cref="PlannedTransfer.Clash"/> for the clashes it leaves whole, so the
/// dialog can describe any of them without touching the disk itself.
/// </para>
/// </remarks>
public sealed class TransferMergeExpander
{
    private readonly ITransferEntrySource _source;

    /// <summary>Directory listings already read, by directory. A drop's sources usually share one
    /// parent, and the destination folder is asked about once per top-level clash.</summary>
    private readonly Dictionary<string, Dictionary<string, TransferEntry>> _listings =
        new(StringComparer.OrdinalIgnoreCase);

    public TransferMergeExpander(ITransferEntrySource source) => _source = source;

    public TransferMergeExpander() : this(new FileSystemTransferProbe())
    {
    }

    public TransferPlan Expand(TransferPlan plan, CancellationToken ct = default)
    {
        if (plan.Conflicts.Count == 0) return plan;

        _listings.Clear();

        var transfers = new List<PlannedTransfer>(plan.Transfers.Count);
        var claimed = new HashSet<string>(StringComparer.Ordinal);
        var pruneDirectories = new List<string>();
        var mergedFolders = new List<string>();

        foreach (var transfer in plan.Transfers)
        {
            ct.ThrowIfCancellationRequested();

            var existing = At(plan.DestinationDirectory, transfer.Name);

            if (!Mergeable(transfer, existing))
            {
                transfers.Add(Whole(transfer, existing, claimed, declined: false));
                continue;
            }

            if (Walk(transfer, claimed, ct) is not { } merged)
            {
                // Too big to put item by item. It keeps the wholesale question it always had, and
                // contributes nothing to the merge — no prune list, and not a merged folder.
                transfers.Add(Whole(transfer, existing, claimed, declined: true));
                continue;
            }

            transfers.AddRange(merged.Transfers);
            pruneDirectories.AddRange(merged.Opened);
            mergedFolders.Add(transfer.Name);
        }

        return plan with
        {
            Transfers = transfers,
            // Deepest first. A child's full path is always longer than its parent's, so ordering by
            // length is enough to guarantee a folder is emptied before it is looked at.
            PruneDirectories = [.. pruneDirectories.OrderByDescending(d => d.Length)],
            MergedFolders = mergedFolders,
        };
    }

    /// <summary>
    /// A folder landing on a folder, with no link on either side. Everything else — a folder onto a
    /// file, a file onto a folder, a file onto a file, or either side a junction — stays one clash
    /// with one answer, exactly as before.
    /// </summary>
    private bool Mergeable(PlannedTransfer transfer, TransferEntry? existing) =>
        transfer.Conflicts &&
        transfer.IsDirectory &&
        existing is { IsDirectory: true, IsReparsePoint: false } &&
        At(Parent(transfer.SourcePath), transfer.Name) is not { IsReparsePoint: true };

    /// <summary>Walks the overlap. Null when it hit a ceiling.</summary>
    private Expansion? Walk(PlannedTransfer folder, HashSet<string> claimed, CancellationToken ct)
    {
        var emitted = new List<PlannedTransfer>();
        var opened = new List<string>();
        var taken = new HashSet<string>(StringComparer.Ordinal);

        var pending = new Stack<(string Source, string Destination)>();
        pending.Push((folder.SourcePath, folder.DestinationPath));

        while (pending.Count > 0)
        {
            ct.ThrowIfCancellationRequested();
            if (opened.Count == TransferMergeLimits.MaxDirectoriesVisited) return null;

            var (sourceDirectory, destinationDirectory) = pending.Pop();

            // Opened rather than moved whole, so a move will have emptied it by the end.
            opened.Add(sourceDirectory);

            foreach (var child in _source.Entries(sourceDirectory))
            {
                ct.ThrowIfCancellationRequested();

                var destinationPath = Path.Combine(destinationDirectory, child.Name);
                var twin = At(destinationDirectory, child.Name);

                // Both sides a real directory: nothing to ask about the folder itself, so open it.
                if (child is { IsDirectory: true, IsReparsePoint: false } &&
                    twin is { IsDirectory: true, IsReparsePoint: false })
                {
                    pending.Push((child.FullPath, destinationPath));
                    continue;
                }

                if (emitted.Count == TransferMergeLimits.MaxExpandedTransfers) return null;

                // Two folders of the same name merging into one destination can both reach the same
                // path, and the planner's own reservations covered only the top level.
                var key = Key(destinationPath);
                var alreadySpokenFor = !taken.Add(key) || claimed.Contains(key);
                var conflicts = twin is not null || alreadySpokenFor;

                emitted.Add(new PlannedTransfer(
                    child.FullPath,
                    child.IsDirectory,
                    destinationPath,
                    conflicts,
                    child.IsDirectory ? null : child.SizeBytes,
                    conflicts ? Clash(Side(child), twin) : null));
            }
        }

        claimed.UnionWith(taken);
        return new Expansion(emitted, opened);
    }

    /// <summary>A clash the expander left whole, described so the dialog need not stat it.</summary>
    private PlannedTransfer Whole(
        PlannedTransfer transfer, TransferEntry? existing, HashSet<string> claimed, bool declined)
    {
        claimed.Add(Key(transfer.DestinationPath));
        if (!transfer.Conflicts) return transfer;

        // The incoming side comes from its own parent's listing, so a folder the merge declined is
        // described as fully as one it expanded — same size and timestamp, from the same source.
        var incoming = At(Parent(transfer.SourcePath), transfer.Name) is { } entry
            ? Side(entry)
            : new ClashSide(transfer.IsDirectory, transfer.KnownBytes ?? 0, default);

        return transfer with
        {
            Clash = Clash(incoming, existing),
            MergeDeclined = declined,
        };
    }

    private static TransferClash Clash(ClashSide incoming, TransferEntry? existing) =>
        existing is { } twin
            ? TransferClash.Between(incoming, Side(twin))
            : TransferClash.Arriving(incoming);

    private static ClashSide Side(TransferEntry entry) =>
        new(entry.IsDirectory, entry.SizeBytes, entry.ModifiedUtc);

    /// <summary>One named entry of a directory, or null. Listings are read once and kept.</summary>
    private TransferEntry? At(string? directory, string name)
    {
        if (directory is null) return null;

        if (!_listings.TryGetValue(directory, out var byName))
        {
            var entries = _source.Entries(directory);
            byName = new Dictionary<string, TransferEntry>(entries.Count, StringComparer.OrdinalIgnoreCase);
            foreach (var entry in entries) byName[entry.Name] = entry;
            _listings[directory] = byName;
        }

        return byName.TryGetValue(name, out var found) ? found : null;
    }

    private static string? Parent(string path)
    {
        try
        {
            return Path.GetDirectoryName(Path.GetFullPath(path));
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return null;
        }
    }

    private static string Key(string path)
    {
        try
        {
            return PathKey.Canonicalize(path);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return path.ToUpperInvariant();
        }
    }

    private sealed record Expansion(IReadOnlyList<PlannedTransfer> Transfers, IReadOnlyList<string> Opened);
}
