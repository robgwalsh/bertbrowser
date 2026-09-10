using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// One long write waiting its turn, running, or finished — the row in the queue list, and the
/// runnable job behind it.
/// </summary>
/// <remarks>
/// The row and the job are one object on purpose. Two parallel lists — one to bind and one to run —
/// is exactly the arrangement that drifts the moment the queue is reordered.
/// </remarks>
public sealed partial class QueuedJobViewModel : ObservableObject
{
    internal QueuedJobViewModel(
        int id, string kind, string description, int items, TransferEstimate? estimate,
        Func<PauseGate, CancellationTokenSource, Task<object?>> body)
    {
        Id = id;
        Kind = kind;
        Description = description;
        Items = items;
        _estimate = estimate;
        Body = body;
    }

    public int Id { get; }

    /// <summary>"Copy", "Move", "Extract", "Compress", "Archive edit" — the noun in the row.</summary>
    public string Kind { get; }

    public string Description { get; }

    public int Items { get; }

    /// <summary>What the job will write, or null while that is not yet known — a compress has to
    /// walk its sources before it can say, and the walk is part of the job. Blank in the row, never
    /// a zero: the same rule the file list follows for a directory with no size row.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeText))]
    private TransferEstimate? _estimate;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StateText))]
    [NotifyPropertyChangedFor(nameof(IsWaiting))]
    private QueuedJobState _state = QueuedJobState.Waiting;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(MoveUpCommand))]
    private bool _canMoveUp;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(MoveDownCommand))]
    private bool _canMoveDown;

    public bool IsWaiting => State == QueuedJobState.Waiting;

    public string SizeText =>
        Estimate is { Complete: true, Bytes: > 0 } known ? ByteSizeFormatter.Format(known.Bytes) : "";

    public string StateText => State switch
    {
        QueuedJobState.Running => "in progress",
        QueuedJobState.Done => "done",
        QueuedJobState.Cancelled => "cancelled",
        _ => "waiting",
    };

    // --- the runnable half ---

    internal Func<PauseGate, CancellationTokenSource, Task<object?>> Body { get; }

    internal CancellationTokenSource Cancellation { get; } = new();

    /// <summary>Completed with whatever the job returned, so the caller that enqueued it awaits its
    /// own outcome exactly as it did when there was no queue. A job cancelled before it ever ran
    /// completes with null, which is what "refused before it began" already meant.</summary>
    internal TaskCompletionSource<object?> Completion { get; } =
        new(TaskCreationOptions.RunContinuationsAsynchronously);

    internal Action<int, int>? Reorder { get; set; }

    internal Action<int>? Stop { get; set; }

    private bool CanExecuteMoveUp() => CanMoveUp;

    private bool CanExecuteMoveDown() => CanMoveDown;

    [RelayCommand(CanExecute = nameof(CanExecuteMoveUp))]
    private void MoveUp() => Reorder?.Invoke(Id, -1);

    [RelayCommand(CanExecute = nameof(CanExecuteMoveDown))]
    private void MoveDown() => Reorder?.Invoke(Id, 1);

    [RelayCommand]
    private void Cancel() => Stop?.Invoke(Id);
}

/// <summary>
/// Every long write this app is doing or about to do, and the one latch that holds them.
/// </summary>
/// <remarks>
/// <para>
/// <b>The queue is presentation and ordering; the draining is the shell's.</b> That is where
/// <c>IsTransferring</c>, <c>RetireUndoable</c> and the undo slots live, and a queue that reached in
/// to touch them would be a second place reasoning about which single operation is currently
/// allowed to write.
/// </para>
/// <para>
/// <b>One gate for the whole queue.</b> Pausing holds the running job at its next chunk boundary,
/// and — because the drain never opens it again on its own — the job after it starts held too.
/// Which is what "pause the queue" has to mean: resuming to find the next four gigabytes had gone
/// past in the meantime is not a pause.
/// </para>
/// </remarks>
public sealed partial class TransferQueueViewModel : ObservableObject
{
    private int _nextId = 1;

    /// <summary>Waiting, running and finished, in the order they will run and did. Finished rows
    /// stay until the queue empties, so a drain of six reads as a list rather than as a flicker.</summary>
    public ObservableCollection<QueuedJobViewModel> Jobs { get; } = [];

    /// <summary>The latch every queued job waits on. Owned here because pause is a property of the
    /// queue rather than of whichever job happens to be running.</summary>
    internal PauseGate Gate { get; } = new();

    /// <summary>
    /// The running job's own progress surface — the same instance
    /// <c>ShellViewModel.TransferProgress</c> holds, so the status strip, the detail window and
    /// this cannot drift.
    /// </summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasRunning))]
    private TransferProgressViewModel? _running;

    [ObservableProperty]
    private bool _isPaused;

    /// <summary>"3 waiting", or empty when the queue holds only what is running.</summary>
    [ObservableProperty]
    private string _waitingText = "";

    public bool HasRunning => Running is not null;

    /// <summary>
    /// True only when a <em>queued</em> job is running. A sync publishes the same kind of surface
    /// without going through the queue — it hosts its own progress inside its dialog — and there is
    /// no gate behind that to hold.
    /// </summary>
    public bool CanPause =>
        Running is { IsCancelling: false } &&
        !IsPaused &&
        Jobs.Any(j => j.State == QueuedJobState.Running);

    public bool CanResume => IsPaused;

    [RelayCommand(CanExecute = nameof(CanPause))]
    private void Pause()
    {
        Gate.Pause();
        IsPaused = true;
        Running?.SetPaused(true);
        Refresh();
    }

    [RelayCommand(CanExecute = nameof(CanResume))]
    private void Resume()
    {
        Gate.Resume();
        IsPaused = false;
        Running?.SetPaused(false);
        Refresh();
    }

    // --- what the drain uses ---

    internal QueuedJobViewModel Add(
        string kind, string description, int items, TransferEstimate? estimate,
        Func<PauseGate, CancellationTokenSource, Task<object?>> body)
    {
        var job = new QueuedJobViewModel(_nextId++, kind, description, items, estimate, body)
        {
            Reorder = Move,
            Stop = Cancel,
        };
        Jobs.Add(job);
        Refresh();
        return job;
    }

    /// <summary>
    /// Puts rows in without anything behind them, so the strip and the queue list can be
    /// photographed at a fixed point.
    /// </summary>
    /// <remarks>
    /// <b>Nothing is queued and nothing runs.</b> A real drain fast enough to be safe in a scripted
    /// run is over before a capture could catch it, and one slow enough to catch would put a
    /// throughput figure into every picture. The same argument as
    /// <see cref="TransferProgressViewModel.PoseForCapture"/>, and the rows are the app's own.
    /// </remarks>
    internal void Pose(
        params (string Kind, string Description, int Items, TransferEstimate? Estimate, QueuedJobState State)[] rows)
    {
        foreach (var row in rows)
        {
            var job = Add(row.Kind, row.Description, row.Items, row.Estimate, (_, _) => Task.FromResult<object?>(null));
            job.State = row.State;
        }

        Refresh();
    }

    internal QueuedJobViewModel? NextWaiting() =>
        Jobs.FirstOrDefault(j => j.State == QueuedJobState.Waiting);

    /// <summary>Clears the record once nothing is left to do. Finished rows are worth keeping while
    /// the queue drains and worth nothing afterwards — the status bar has already said how each one
    /// turned out.</summary>
    internal void Clear()
    {
        Jobs.Clear();
        IsPaused = false;
        Refresh();
    }

    /// <summary>Recomputes everything derived from the list. Called after any change to it, because
    /// an <see cref="ObservableCollection{T}"/> reports that it changed and not what that means.</summary>
    internal void Refresh()
    {
        var summaries = Summaries();
        WaitingText = TransferQueueRules.WaitingText(summaries);

        foreach (var job in Jobs)
        {
            job.CanMoveUp = TransferQueueRules.CanMoveUp(summaries, job.Id);
            job.CanMoveDown = TransferQueueRules.CanMoveDown(summaries, job.Id);
        }

        OnPropertyChanged(nameof(CanPause));
        OnPropertyChanged(nameof(CanResume));
        PauseCommand.NotifyCanExecuteChanged();
        ResumeCommand.NotifyCanExecuteChanged();
    }

    /// <summary>What the whole queue still has to write, for anything that wants to say so.</summary>
    internal TransferEstimate Remaining() => TransferQueueRules.Remaining(Summaries());

    private IReadOnlyList<QueuedJobSummary> Summaries() =>
    [
        .. Jobs.Select(j => new QueuedJobSummary(
            j.Id, j.Kind, j.Description, j.Items,
            j.Estimate ?? new TransferEstimate(0, 0, Complete: false), j.State)),
    ];

    private void Move(int id, int delta)
    {
        var ordered = TransferQueueRules.Move(Summaries(), id, delta);
        var byId = Jobs.ToDictionary(j => j.Id);

        for (var i = 0; i < ordered.Count; i++)
        {
            var wanted = byId[ordered[i].Id];
            if (!ReferenceEquals(Jobs[i], wanted)) Jobs.Move(Jobs.IndexOf(wanted), i);
        }

        Refresh();
    }

    /// <summary>
    /// Stops one job. A waiting one never runs and completes as "refused before it began"; the
    /// running one is cancelled through its own token — and the gate is opened, because a run held
    /// at a chunk boundary cannot notice a cancel it is not awake for.
    /// </summary>
    private void Cancel(int id)
    {
        if (Jobs.FirstOrDefault(j => j.Id == id) is not { } job) return;

        switch (job.State)
        {
            case QueuedJobState.Waiting:
                job.State = QueuedJobState.Cancelled;
                job.Completion.TrySetResult(null);

                // Nothing will ever run this, so nothing else will dispose its token source. The
                // drain does that for the jobs it takes.
                job.Cancellation.Dispose();
                break;

            case QueuedJobState.Running:
                job.Cancellation.Cancel();
                if (IsPaused) Resume();
                break;
        }

        Refresh();
    }
}
