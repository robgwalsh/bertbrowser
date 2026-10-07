using System.IO;
using BertBrowser.Core.Services.Metadata;
using BertBrowser.Core.Services.Timestamps;
using BertBrowser.Core.Services.Transfer;
using BertBrowser.Core.Services.UndoHistory;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// Editing the metadata inside files — a picture's EXIF, a song's tags.
/// </summary>
/// <remarks>
/// <b>The editor window never writes.</b> It plans through here and executes through here, which is
/// what puts an edit behind the same queue, pause, progress surface and undo history as every other
/// write. A window with its own executor would be a second writer nothing serialises.
/// </remarks>
public sealed partial class ShellViewModel
{
    public MetadataEditPlan PlanMetadataEdit(
        IReadOnlyList<string> paths,
        MetadataEdit edit,
        IReadOnlyDictionary<string, IReadOnlySet<MetadataField>>? viaWindows = null) =>
        _metadataPlanner.Plan(paths, edit, viaWindows);

    /// <summary>
    /// Carries out a metadata edit, on the transfer progress surface and into the undo history.
    /// </summary>
    /// <returns>Null when there was nothing to do or the app was busy with an unqueued write.</returns>
    /// <remarks>
    /// Queued, not run under the flag like a rename: every file is written out again whole, so
    /// retitling a folder of videos is a long write however small the change.
    /// </remarks>
    public async Task<MetadataEditOutcome?> EditMetadataAsync(MetadataEditPlan plan)
    {
        if (!plan.HasWork) return null;

        var estimate = new TransferEstimate(plan.RewriteBytes, plan.Files.Count, Complete: true);
        var description = plan.Files.Count == 1
            ? Path.GetFileName(plan.Files[0].Path)
            : $"{plan.Files.Count} files";

        return await EnqueueAsync<MetadataEditOutcome>(
            "Metadata edit",
            description,
            plan.Files.Count,
            estimate,
            (gate, cancellation) => RunMetadataEditAsync(plan, estimate, description, gate, cancellation));
    }

    private async Task<MetadataEditOutcome?> RunMetadataEditAsync(
        MetadataEditPlan plan,
        TransferEstimate estimate,
        string description,
        PauseGate gate,
        CancellationTokenSource cancellation)
    {
        var folders = plan.Files
            .Select(f => Path.GetDirectoryName(f.Path))
            .OfType<string>()
            .Where(d => d.Length > 0)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        var synthetic = new TransferPlan(
            TransferVerb.Copy, folders.FirstOrDefault() ?? "",
            [.. plan.Files.Select(f => new PlannedTransfer(f.Path, false, f.Path, false))], []);

        var surface = new TransferProgressViewModel(synthetic, estimate, cancellation.Cancel);
        surface.Headline = $"Writing metadata to {description}…";
        TransferProgress = surface;
        SetStatus(surface.Headline);

        var progress = new Progress<TransferProgress>(surface.Apply);
        var outcome = await Task.Run(
            () => _metadataExecutor.Execute(plan, cancellation.Token, progress, gate));

        await RecordAsync(MetadataEditRecord.For(outcome));
        await RefreshTabsShowingAsync(folders);

        SetStatus(Describe(outcome));
        return outcome;
    }

    /// <summary>
    /// The same sources with each file's own metadata read onto it, for a rename template that
    /// names a tag. A file that cannot be read gets an empty set, which the template then refuses
    /// for that item by name.
    /// </summary>
    public IReadOnlyList<BertBrowser.Core.Services.Rename.RenameSource> ReadRenameTags(
        IReadOnlyList<BertBrowser.Core.Services.Rename.RenameSource> sources) =>
        [.. sources.Select(s => s with { Tags = TagsOf(s.Path, s.IsDirectory) })];

    private static IReadOnlyDictionary<MetadataField, string> TagsOf(string path, bool isDirectory)
    {
        if (isDirectory || MetadataEditPlanner.IsInsideArchive(path) ||
            MetadataCodecs.For(path) is not { } codec)
            return new Dictionary<MetadataField, string>();

        try
        {
            using var stream = BertBrowser.Core.Services.ReadOnlyFile.TryOpen(path);
            return stream is null ? new Dictionary<MetadataField, string>() : codec.Read(stream).Values;
        }
        catch (MetadataFormatException)
        {
            return new Dictionary<MetadataField, string>();
        }
        catch (Exception ex) when (BertBrowser.Core.Services.ReadOnlyFile.IsReadFailure(ex))
        {
            return new Dictionary<MetadataField, string>();
        }
    }

    public TimestampPlan PlanTimestamps(IReadOnlyList<string> paths, TimestampChange change) =>
        _timestampPlanner.Plan(paths, change);

    /// <summary>
    /// Sets the dates a plan describes, and records the change in the undo history.
    /// </summary>
    /// <returns>Null when there was nothing to do or another write was under way.</returns>
    /// <remarks>
    /// Run under the flag like a rename rather than queued: a date is a metadata write of a few
    /// bytes per item, and nothing about it is long enough to pause.
    /// </remarks>
    public async Task<TimestampOutcome?> SetTimestampsAsync(TimestampPlan plan)
    {
        if (IsTransferring || !plan.HasWork) return null;

        IsTransferring = true;
        UndoCommand.NotifyCanExecuteChanged();
        try
        {
            var outcome = await Task.Run(() => _timestampExecutor.Execute(plan));
            await RecordAsync(TimestampRecord.For(outcome));

            await RefreshTabsShowingAsync(plan.Items
                .Select(i => Path.GetDirectoryName(i.Path))
                .OfType<string>()
                .Where(d => d.Length > 0)
                .Distinct(StringComparer.OrdinalIgnoreCase));

            var changed = outcome.Stamped.Count == 1
                ? $"The date of {Path.GetFileName(outcome.Stamped[0].Path)} was changed"
                : $"The dates of {outcome.Stamped.Count} items were changed";

            SetStatus(outcome.Failed.Count switch
            {
                0 => changed + ".",
                1 when outcome.Stamped.Count == 0 => outcome.Failed[0].Message,
                var n => $"{changed}; {n} could not be.",
            });

            return outcome;
        }
        finally
        {
            IsTransferring = false;
            UndoCommand.NotifyCanExecuteChanged();
        }
    }

    private static string Describe(MetadataEditOutcome outcome)
    {
        var changed = outcome.Edited.Count == 1
            ? $"{Path.GetFileName(outcome.Edited[0].Path)} updated"
            : $"{outcome.Edited.Count} files updated";

        if (outcome.Failed.Count == 1 && outcome.Edited.Count == 0) return outcome.Failed[0].Message;
        if (outcome.Failed.Count > 0) return $"{changed}; {outcome.Failed.Count} could not be changed.";
        if (outcome.Cancelled) return outcome.Edited.Count == 0 ? "Nothing was changed." : $"{changed}, then stopped.";
        return changed + ".";
    }
}
