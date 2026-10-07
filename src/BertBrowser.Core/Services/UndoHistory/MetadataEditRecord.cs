using BertBrowser.Core.Services.Metadata;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// A metadata edit in the undo history. Each file has its other version held beside it — the
/// original while the edit is in effect, the edited one while it is undone — and a step swaps the
/// two, so undo and redo are the same move in opposite directions.
/// </summary>
/// <remarks>
/// A file that would not swap drops out of the record, as with every other kind, and its held
/// version is <em>left where it is</em> rather than erased: the failure names the path, and a
/// stray file beside a picture is a far smaller problem than the only copy of its original gone.
/// </remarks>
public sealed class MetadataEditRecord : IUndoableRecord
{
    private readonly IReadOnlyList<HeldVersion> _held;

    private MetadataEditRecord(IReadOnlyList<HeldVersion> held, bool isApplied, string description)
    {
        _held = held;
        IsApplied = isApplied;
        Description = description;
    }

    public static MetadataEditRecord? For(MetadataEditOutcome outcome) =>
        outcome.CanUndo
            ? new MetadataEditRecord(outcome.Edited, true, Describe(outcome.Edited))
            : null;

    public UndoKind Kind => UndoKind.MetadataEdit;

    public bool IsApplied { get; }

    public string Description { get; }

    public int ItemCount => _held.Count;

    public IReadOnlyList<UndoItemDetail> Details =>
        [.. _held.Select(h => new UndoItemDetail(h.Path, h.Path, IsDirectory: false))];

    public IReadOnlyList<HeldEntry> Held =>
        [.. _held.Select(h => new HeldEntry(h.HeldPath, h.Path, IsDirectory: false))];

    public async Task<StepResult> StepAsync(IUndoHost host)
    {
        var held = _held;
        var result = await Task.Run(() => IsApplied ? host.MetadataEdits.Undo(held) : host.MetadataEdits.Redo(held));

        var refresh = UndoRefresh.Of(held.Select(h => Path.GetDirectoryName(h.Path)));
        var report = new StepReport(result.Swapped.Count, [.. result.Failed.Select(f => f.Message)]);

        return result.Swapped.Count == 0
            ? new StepResult(this, report, refresh)
            : new StepResult(new MetadataEditRecord(result.Swapped, !IsApplied, Description), report, refresh);
    }

    public void Retire() => MetadataEditExecutor.CommitStaging(_held);

    private static string Describe(IReadOnlyList<HeldVersion> edited) =>
        edited.Count == 1
            ? $"Edit metadata of {Path.GetFileName(edited[0].Path)}"
            : $"Edit metadata of {edited.Count} files";
}
