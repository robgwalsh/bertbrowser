using BertBrowser.Core.Services.Archives;

namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>
/// An edit to an archive's contents in the undo history. Both directions are a swap of two whole
/// containers beside each other, so each state holds exactly one file: the original while the edit
/// is in effect, the edited one while it is undone.
/// </summary>
public sealed class ArchiveEditRecord : IUndoableRecord
{
    private readonly ArchiveEditOutcome? _applied;
    private readonly ArchiveEditUndo? _undone;

    private ArchiveEditRecord(ArchiveEditOutcome? applied, ArchiveEditUndo? undone, string description)
    {
        _applied = applied;
        _undone = undone;
        Description = description;
    }

    public static ArchiveEditRecord? For(ArchiveEditOutcome outcome) =>
        outcome.CanUndo
            ? new ArchiveEditRecord(outcome, null, $"Edit {Path.GetFileName(outcome.ArchiveFile)}")
            : null;

    private string ArchiveFile => _applied?.ArchiveFile ?? _undone!.ArchiveFile;

    public UndoKind Kind => UndoKind.ArchiveEdit;

    public bool IsApplied => _applied is not null;

    public string Description { get; }

    public int ItemCount => 1;

    public IReadOnlyList<UndoItemDetail> Details =>
        [new UndoItemDetail(ArchiveFile, ArchiveFile, IsDirectory: false)];

    public IReadOnlyList<HeldEntry> Held =>
        (_applied?.StagedOriginal ?? _undone?.StagedEdited) is { } held
            ? [new HeldEntry(held, ArchiveFile, IsDirectory: false)]
            : [];

    public async Task<StepResult> StepAsync(IUndoHost host)
    {
        var archive = ArchiveFile;
        var refresh = UndoRefresh.Of([Path.GetDirectoryName(archive)]) with { Containers = [archive] };

        if (_applied is { } applied)
        {
            var undo = await Task.Run(() => host.ArchiveEdits.Undo(applied));
            return undo.Failure is { } failure
                ? new StepResult(this, new StepReport(0, [failure]), refresh)
                : new StepResult(new ArchiveEditRecord(null, undo, Description), new StepReport(1, []), refresh);
        }

        var redo = await Task.Run(() => host.ArchiveEdits.Redo(_undone!));
        return redo.Failure is { } refused
            ? new StepResult(this, new StepReport(0, [refused]), refresh)
            : new StepResult(new ArchiveEditRecord(redo, null, Description), new StepReport(1, []), refresh);
    }

    public void Retire()
    {
        if (_applied is { } applied) ArchiveEditExecutor.CommitStaging(applied);
        else if (_undone is { } undone) ArchiveEditExecutor.CommitStaging(undone);
    }
}
