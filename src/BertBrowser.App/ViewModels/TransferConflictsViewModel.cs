using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>One name that already exists at the destination, with both sides described so the
/// user can tell which copy is which before deciding — and its own answer.</summary>
public sealed partial class TransferConflictViewModel : ObservableObject
{
    public string Name { get; }
    public string SourcePath { get; }
    public string IncomingDetail { get; }
    public string ExistingDetail { get; }
    public string ExistingLabel { get; }

    /// <summary>Highlighted in the dialog: the common case for wanting Replace.</summary>
    public bool IncomingIsNewer { get; }

    /// <summary>Both sides are the same file. Said out loud, because a row pre-set to Skip with no
    /// explanation looks like the dialog deciding something on its own.</summary>
    public bool IsIdentical { get; }

    public string IdenticalNote => IsIdentical ? "identical — same size and date" : "";

    /// <summary>Non-empty when this folder was too large to put item by item, so the single answer
    /// on this row settles everything inside it.</summary>
    public string MergeNote { get; }

    /// <summary>Replace is offered for a move, and inside a merge — see
    /// <see cref="ConflictDefaults.AllowsReplace"/>.</summary>
    public bool AllowReplace { get; }

    /// <summary>
    /// What groups this row's three radio buttons. The canonical <em>source</em> path, because a
    /// group name has to be unique per row and a display name is not: two sources from different
    /// folders can carry the same name — which is one of the ways a row gets here in the first
    /// place, and after a merge two rows can share a leaf name outright.
    /// </summary>
    public string GroupName { get; }

    /// <summary>This clash's own answer, from <see cref="ConflictDefaults"/> so the offered starting
    /// point and the one an unattended run takes cannot drift apart.</summary>
    [ObservableProperty]
    private ConflictResolution _resolution;

    public TransferConflictViewModel(TransferPlan plan, PlannedTransfer transfer)
    {
        Name = plan.LabelFor(transfer);
        SourcePath = transfer.SourcePath;
        GroupName = PathKey.Canonicalize(transfer.SourcePath);
        AllowReplace = ConflictDefaults.AllowsReplace(plan, transfer);
        _resolution = ConflictDefaults.For(transfer);
        MergeNote = transfer.MergeDeclined
            ? $"Too large to merge item by item (over {TransferMergeLimits.MaxExpandedTransfers:N0} " +
              "entries), so this answer applies to the whole folder."
            : "";

        // A plan that went through the expander already knows both sides, and describing them from
        // it rather than from disk is what keeps a merged folder's worth of rows off the UI thread.
        // Anything else — a sync's plan, an archive's, the elevation host's — still has to look.
        if (transfer.Clash is { } clash)
        {
            IncomingDetail = Describe(clash.Incoming);
            IsIdentical = clash.Identical;
            IncomingIsNewer = clash.IncomingIsNewer;

            if (clash.Existing is { } existing)
            {
                ExistingLabel = "Already here:";
                ExistingDetail = IsIdentical ? IdenticalNote : Describe(existing);
            }
            else if (plan.EarlierClaimantOf(transfer) is { } earlier)
            {
                ExistingLabel = "Also arriving:";
                ExistingDetail = $"{earlier.Name} from {Path.GetDirectoryName(earlier.SourcePath)}";
            }
            else
            {
                ExistingLabel = "Already here:";
                ExistingDetail = "missing";
            }

            return;
        }

        var incoming = DescribeOnDisk(transfer.SourcePath, transfer.IsDirectory, out var incomingTime);
        IncomingDetail = incoming;

        // Two clashes are possible and they need different words. Something already on disk is what
        // a Replace would displace; an earlier item in this same transfer is a choice between two
        // incoming copies, and asking disk about it answers "missing" — true, and no help at all.
        if (File.Exists(transfer.DestinationPath) || Directory.Exists(transfer.DestinationPath))
        {
            ExistingLabel = "Already here:";
            ExistingDetail = DescribeOnDisk(
                transfer.DestinationPath, Directory.Exists(transfer.DestinationPath), out var existingTime);
            IncomingIsNewer = incomingTime > existingTime;
        }
        else if (plan.EarlierClaimantOf(transfer) is { } earlier)
        {
            ExistingLabel = "Also arriving:";
            ExistingDetail = $"{earlier.Name} from {Path.GetDirectoryName(earlier.SourcePath)}";
        }
        else
        {
            ExistingLabel = "Already here:";
            ExistingDetail = "missing";
        }
    }

    private static string Describe(ClashSide side)
    {
        if (side.IsDirectory)
            return side.ModifiedUtc == default
                ? "folder"
                : $"folder — modified {side.ModifiedUtc.ToLocalTime():g}";

        var size = ByteSizeFormatter.Format(side.Bytes);
        return side.ModifiedUtc == default
            ? size
            : $"{size} — modified {side.ModifiedUtc.ToLocalTime():g}";
    }

    private static string DescribeOnDisk(string path, bool isDirectory, out DateTime modified)
    {
        modified = DateTime.MinValue;
        try
        {
            if (isDirectory)
            {
                var dir = new DirectoryInfo(path);
                if (!dir.Exists) return "missing";
                modified = dir.LastWriteTime;
                return $"folder — modified {modified:g}";
            }

            var file = new FileInfo(path);
            if (!file.Exists) return "missing";
            modified = file.LastWriteTime;
            return $"{ByteSizeFormatter.Format(file.Length)} — modified {modified:g}";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return "unreadable";
        }
    }
}

/// <summary>
/// Backing VM for <see cref="Views.TransferConflictDialog"/>: every clash, each with its own answer.
/// </summary>
/// <remarks>
/// <para>
/// <b>Per item, with a way to answer the lot.</b> A drop of forty files onto forty taken names is
/// almost always one decision, so "Apply to all" has to stay one gesture; but the case the single
/// answer could not express — keep this one, skip that one, replace the third — is the one worth
/// opening a dialog for at all.
/// </para>
/// <para>
/// <b>A merged folder is not itself a row.</b> A folder landing on a folder of the same name is
/// merged rather than settled wholesale, so what reaches this list is the items <em>inside</em> it
/// that really clash, each named by its path relative to the drop. Everything else in the folder is
/// carried across without a question, which is the whole point.
/// </para>
/// <para>
/// The answer comes out as the per-path map the executor has always taken, so nothing between here
/// and <c>TransferExecutor</c> has to know what a radio button is. The same shape
/// <c>SyncPreviewViewModel.Result</c> uses.
/// </para>
/// </remarks>
public sealed partial class TransferConflictsViewModel : ObservableObject
{
    private readonly TransferPlan _plan;

    public IReadOnlyList<TransferConflictViewModel> Items { get; }
    public bool AllowReplace { get; }
    public string Title { get; }
    public string Summary { get; }
    public string Subtitle { get; }
    public string ReplaceHint { get; }

    public TransferConflictsViewModel(TransferPlan plan)
    {
        _plan = plan;
        Items = [.. plan.Conflicts.Select(c => new TransferConflictViewModel(plan, c))];
        AllowReplace = Items.Any(i => i.AllowReplace);

        var folder = Path.GetFileName(plan.DestinationDirectory) is { Length: > 0 } name
            ? name
            : plan.DestinationDirectory;
        var verbing = plan.Verb == TransferVerb.Move ? "moving" : "copying";
        var identical = Items.Count(i => i.IsIdentical);

        if (plan.IsMerged)
        {
            var merged = plan.MergedFolders.Count == 1
                ? $"'{plan.MergedFolders[0]}'"
                : $"{plan.MergedFolders.Count:N0} folders";
            Title = plan.MergedFolders.Count == 1
                ? $"Merging {plan.MergedFolders[0]} into {folder}"
                : $"Merging {plan.MergedFolders.Count:N0} folders into {folder}";
            Summary = Items.Count == 1
                ? $"{merged} already exists in {folder}. One item inside already exists too."
                : $"{merged} already exists in {folder}. {Items.Count:N0} of the items inside already exist too.";
            Subtitle = "Choose what to do with each. Everything else is " +
                (plan.Verb == TransferVerb.Move ? "moved" : "copied") + " across without asking.";
        }
        else
        {
            Title = Items.Count == 1 ? "An item already exists" : $"{Items.Count:N0} items already exist";
            Summary = Items.Count == 1
                ? $"'{Items[0].Name}' already exists in {folder}."
                : $"{Items.Count:N0} of the items you are {verbing} already exist in {folder}.";
            Subtitle = "Choose what to do with each of them.";
        }

        if (identical > 0)
            Subtitle += identical == 1
                ? " One is the same file on both sides and is set to skip."
                : $" {identical:N0} are the same file on both sides and are set to skip.";

        ReplaceHint = "Replace keeps the incoming copy. What it displaces is set aside, and Ctrl+Z "
            + "restores both sides until you do something else.";
    }

    /// <summary>
    /// The answer, keyed by canonical source path exactly as <c>TransferExecutor.Execute</c> wants
    /// it. Only the clashing items appear; everything else falls back to Keep both there.
    /// </summary>
    /// <remarks>
    /// A chosen Replace is translated on the way out, because what it means depends on the verb: a
    /// move displaces through <see cref="ConflictResolution.Replace"/>, a merged copy through
    /// <see cref="ConflictResolution.Overwrite"/>. Doing it here keeps the rows, the radio buttons
    /// and "Apply to all" all speaking about one idea.
    /// </remarks>
    public IReadOnlyDictionary<string, ConflictResolution> Resolutions =>
        Items.ToDictionary(
            i => i.GroupName,
            i => i.Resolution == ConflictResolution.Replace
                ? ConflictDefaults.ReplaceMeans(_plan)
                : i.Resolution);

    /// <summary>Sets every row at once — the whole of what this dialog used to be able to say, now
    /// one press instead of the only press.</summary>
    [RelayCommand]
    private void ApplyToAll(ConflictResolution resolution)
    {
        foreach (var item in Items)
        {
            if (resolution == ConflictResolution.Replace && !item.AllowReplace) continue;
            item.Resolution = resolution;
        }
    }
}
