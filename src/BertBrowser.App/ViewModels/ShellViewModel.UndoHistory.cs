using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using BertBrowser.Core.Services.UndoHistory;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// The undo history: every move, rename, delete, merged paste, archive edit and sync this session,
/// undoable and redoable in order.
/// </summary>
/// <remarks>
/// <para>
/// <b>Recording is where the old single slot used to retire.</b> Each operation still hands its
/// outcome over after the elevated retry and before the refresh; <see cref="RecordAsync"/> pushes it
/// and retires whatever the push released — an abandoned redo branch, or the oldest entries the
/// budget no longer has room for. That retirement is the moment data goes, exactly as
/// the old single slot's retire was.
/// </para>
/// <para>
/// A walk through the history — one step or twenty — holds <see cref="IsTransferring"/> for all of
/// it, so nothing can be written between two of its steps and nothing can push onto the stack out
/// from under it.
/// </para>
/// </remarks>
public sealed partial class ShellViewModel
{
    private UndoStack _undo = null!;
    private HeldSizeEstimator _heldSizes = null!;
    private IUndoHost _undoHost = null!;

    /// <summary>The history as the window and the menus show it. Always present.</summary>
    public UndoHistoryViewModel History { get; private set; } = null!;

    /// <summary>Raised to open the History window.</summary>
    public event Action? UndoHistoryRequested;

    /// <summary>"Ctrl+Z: undo move 3 items to Documents"; empty when there is nothing to undo.</summary>
    [ObservableProperty]
    private string _undoDescription = "";

    /// <summary>"Ctrl+Y: redo rename 'a.txt' to 'b.txt'"; empty when there is nothing to redo.</summary>
    [ObservableProperty]
    private string _redoDescription = "";

    public bool CanUndo => _undo.NextUndo is not null && !IsTransferring;

    public bool CanRedo => _undo.NextRedo is not null && !IsTransferring;

    /// <summary>The history itself, for the harness and the menus.</summary>
    public UndoStack UndoStack => _undo;

    private void InitializeUndoHistory()
    {
        _undo = new UndoStack(_settings.EffectiveUndoBudget());
        _heldSizes = new HeldSizeEstimator(new IndexedHeldFolderSizes(_dirSizes));
        _undoHost = new UndoHost(this);
        History = new UndoHistoryViewModel(UndoToAsync, RedoToAsync);
    }

    partial void OnIsTransferringChanged(bool value) => UpdateUndoState();

    // --- recording ---

    /// <summary>
    /// Pushes a finished operation onto the history, or does nothing when it cannot be taken back.
    /// </summary>
    private async Task RecordAsync(IUndoableRecord? record)
    {
        if (record is null) return;

        var held = await Task.Run(() => _heldSizes.Measure(record));
        var (_, released) = _undo.Push(record, held, DateTime.UtcNow);
        await RetireAsync(released);
        UpdateUndoState();
    }

    private static Task RetireAsync(Released released) =>
        released.Entries.Count == 0 ? Task.CompletedTask : Task.Run(released.RetireAll);

    // --- stepping ---

    /// <summary>Reverses the last operation still in effect.</summary>
    /// <summary>What a status line adds after something undoable. Asked of the keymap each time,
    /// because Undo can be rebound — or left with no key at all.</summary>
    private static string UndoHint =>
        GestureText.For("edit.undo") is { Length: > 0 } gesture ? $" — {gesture} to undo" : " — this can be undone";

    /// <summary>"Ctrl+Z: undo moving 3 items", or the sentence alone when the command has no key.</summary>
    private static string Described(string commandId, string what) =>
        GestureText.For(commandId) is { Length: > 0 } gesture
            ? $"{gesture}: {what}"
            : char.ToUpperInvariant(what[0]) + what[1..];

    [RelayCommand(CanExecute = nameof(CanUndo))]
    private Task UndoAsync() =>
        _undo.NextUndo is { } entry ? UndoToAsync(entry) : Task.CompletedTask;

    /// <summary>Does the last undone operation again.</summary>
    [RelayCommand(CanExecute = nameof(CanRedo))]
    private Task RedoAsync() =>
        _undo.NextRedo is { } entry ? RedoToAsync(entry) : Task.CompletedTask;

    /// <summary>Undoes <paramref name="target"/> and everything after it, newest first.</summary>
    public Task UndoToAsync(UndoEntry target) =>
        _undo.Entries.Contains(target) && target.IsDone
            ? WalkAsync(_undo.UndoSpan(target), UndoDirection.Undo)
            : Task.CompletedTask;

    /// <summary>Redoes everything up to and including <paramref name="target"/>, oldest first.</summary>
    public Task RedoToAsync(UndoEntry target) =>
        _undo.Entries.Contains(target) && !target.IsDone
            ? WalkAsync(_undo.RedoSpan(target), UndoDirection.Redo)
            : Task.CompletedTask;

    private async Task WalkAsync(IReadOnlyList<UndoEntry> span, UndoDirection direction)
    {
        if (IsTransferring || span.Count == 0) return;

        IsTransferring = true;
        try
        {
            SetStatus(direction == UndoDirection.Undo ? "Undoing…" : "Redoing…");
            var result = await UndoNavigator.WalkAsync(
                _undo, span, _undoHost, _heldSizes.Measure, () => DateTime.UtcNow);

            await RetireAsync(result.Released);
            await ApplyRefreshesAsync(result.Refreshes);
            SetStatus(DescribeWalk(span, direction, result));
        }
        finally
        {
            IsTransferring = false;
        }
    }

    /// <summary>
    /// "Undone — move 3 items to Documents", "Undone 4 actions", or where and why a walk stopped.
    /// </summary>
    private static string DescribeWalk(IReadOnlyList<UndoEntry> span, UndoDirection direction, NavigationResult result)
    {
        var did = direction == UndoDirection.Undo ? "Undone" : "Redone";
        var verb = direction == UndoDirection.Undo ? "undo" : "redo";
        var notes = string.Concat(result.Notes);

        if (result.StoppedAt is not { } stopped || result.StopReport is not { } report)
        {
            return (span.Count == 1
                ? $"{did} — {UndoText.AsObject(span[0].Record.Description)}"
                : $"{did} {span.Count:N0} actions") + notes;
        }

        var name = UndoText.AsObject(stopped.Record.Description);
        var text = report.NothingHappened
            ? $"Could not {verb} {name} — {report.Failures[0]}"
            : $"{did} {name} only in part: {report.Failures.Count:N0} could not be — {report.Failures[0]}";
        var remaining = span.Count - result.StepsTaken - (report.NothingHappened ? 1 : 0);
        if (remaining > 0) text += $" (stopped there; {remaining:N0} more not {did.ToLowerInvariant()})";
        return text + notes;
    }

    /// <summary>Reloads everything the steps left stale, each folder once however many steps
    /// touched it.</summary>
    private async Task ApplyRefreshesAsync(IReadOnlyList<UndoRefresh> refreshes)
    {
        foreach (var refresh in refreshes)
        {
            // Vacate first: a tab inside a folder that has just gone would otherwise be reloaded
            // onto a missing path on its way out of it.
            if (refresh.Vacated.Count > 0) await LeaveDeletedFoldersAsync(refresh.Vacated);
            if (refresh.Renamed.Count > 0) await FollowRenamedFoldersAsync(refresh.Renamed);
        }

        var directories = refreshes
            .SelectMany(r => r.Directories)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
        await Tree.RefreshDirectoriesAsync(directories);
        await RefreshTabsShowingAsync(directories);

        foreach (var container in refreshes.SelectMany(r => r.Containers).Distinct(StringComparer.OrdinalIgnoreCase))
            await RefreshTabsUnderAsync(container);

        // The verdicts described the folders as they were a moment ago and no longer do.
        if (refreshes.Any(r => r.RescanCompare)) CompareSession?.RescanCommand.Execute(null);
    }

    // --- the history itself ---

    [RelayCommand]
    private void OpenUndoHistory() => UndoHistoryRequested?.Invoke();

    /// <summary>
    /// Commits everything the history holds and empties it. The caller asks first — the History
    /// window says how much will be committed.
    /// </summary>
    public async Task ClearUndoHistoryAsync()
    {
        if (IsTransferring) return;

        await RetireAsync(_undo.Clear());
        UpdateUndoState();
        SetStatus("Undo history cleared.");
    }

    /// <summary>
    /// Commits everything the history holds, at once, on this thread. For the window closing: the
    /// history is session-only, and this is the moment every set-aside file finally goes.
    /// </summary>
    public void ReleaseUndoHistory()
    {
        _undo.Clear().RetireAll();
        UpdateUndoState();
    }

    /// <summary>Applies new limits from Settings, releasing whatever no longer fits.</summary>
    public async Task SetUndoBudgetAsync(UndoBudget budget)
    {
        var released = _undo.SetBudget(budget);
        await RetireAsync(released);
        UpdateUndoState();
    }

    /// <summary>Brings the commands, the descriptions and the history view into line with the stack.</summary>
    private void UpdateUndoState()
    {
        // Called from the IsTransferring change handler, which the generated property raises once
        // during construction-time seeding in principle; the stack does not exist until then.
        if (_undo is null) return;

        UndoDescription = _undo.NextUndo is { } undo
            ? Described("edit.undo", $"undo {UndoText.AsObject(undo.Record.Description)}")
            : "";
        RedoDescription = _undo.NextRedo is { } redo
            ? Described("edit.redo", $"redo {UndoText.AsObject(redo.Record.Description)}")
            : "";

        UndoCommand.NotifyCanExecuteChanged();
        RedoCommand.NotifyCanExecuteChanged();
        History.Refresh(_undo, DateTime.UtcNow, canStep: !IsTransferring);
    }

    /// <summary>
    /// What a record needs from the shell: the executors, and the same elevated retries and
    /// progress surface the operations themselves use.
    /// </summary>
    /// <remarks>
    /// A nested class so it can reach the private <c>ElevateIfRefusedAsync</c> overloads without them
    /// becoming part of the shell's surface. There is no member for retrying a copy's undo — see
    /// <see cref="IUndoHost"/>.
    /// </remarks>
    private sealed class UndoHost(ShellViewModel shell) : IUndoHost
    {
        public TransferExecutor Transfers => shell._transferExecutor;

        public DeleteExecutor Deletes => shell._deleteExecutor;

        public RenameExecutor Renames => shell._renameExecutor;

        public ArchiveEditExecutor ArchiveEdits => shell._archiveEditExecutor;

        public SyncRunner Sync => shell._syncRunner;

        public Task<Elevated<TransferOutcome>> ElevateAsync(
            TransferPlan plan, TransferOutcome outcome, IReadOnlyDictionary<string, ConflictResolution> resolutions) =>
            shell.ElevateIfRefusedAsync(plan, outcome, resolutions);

        public Task<Elevated<DeleteOutcome>> ElevateAsync(DeletePlan plan, DeleteOutcome outcome) =>
            shell.ElevateIfRefusedAsync(plan, outcome);

        public Task<Elevated<RenameOutcome>> ElevateAsync(RenamePlan plan, RenameOutcome outcome) =>
            shell.ElevateIfRefusedAsync(plan, outcome);

        public Task<Elevated<TransferUndoResult>> ElevateMoveUndoAsync(TransferOutcome outcome, TransferUndoResult result)
        {
            // ElevationHost.UndoTransfer hardcodes a move. Handed a copy's outcome it would move the
            // pasted files back into the folder they came from instead of removing them.
            if (outcome.Verb != TransferVerb.Move)
                throw new InvalidOperationException("Only a move's undo can be retried elevated.");
            return shell.ElevateIfRefusedAsync(outcome, result);
        }

        public Task<Elevated<DeleteUndoResult>> ElevateDeleteUndoAsync(DeleteOutcome outcome, DeleteUndoResult result) =>
            shell.ElevateIfRefusedAsync(outcome, result);

        public IProgress<TransferProgress>? BeginProgress(TransferPlan plan, string headline)
        {
            var estimate = TransferEstimator.Estimate(plan, IndexedTransferSizeSource.For(plan, shell._dirSizes));
            var surface = new TransferProgressViewModel(plan, estimate, () => { }) { Headline = headline };
            shell.TransferProgress = surface;
            shell.SetStatus(headline);
            return new Progress<TransferProgress>(surface.Apply);
        }

        public void EndProgress() => shell.TransferProgress = null;
    }
}
