using System;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Linq;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>What one item in a running transfer is doing.</summary>
public enum TransferItemState
{
    Waiting,
    Working,
    Done,
}

/// <summary>One row of the detail window.</summary>
public sealed partial class TransferItemRow : ObservableObject
{
    public TransferItemRow(TransferPlan plan, PlannedTransfer transfer)
    {
        Name = plan.LabelFor(transfer);
        Destination = System.IO.Path.GetDirectoryName(transfer.DestinationPath) ?? "";
    }

    public string Name { get; }

    public string Destination { get; }

    [ObservableProperty]
    private TransferItemState _state = TransferItemState.Waiting;
}

/// <summary>
/// The live state of one transfer, shared by the status bar and the detail window so the two
/// cannot drift apart. Fed by <see cref="Apply"/> from the executor's progress reports.
/// </summary>
/// <remarks>
/// <b>Every derived figure is withheld rather than invented.</b> Without a complete byte total from
/// the size index there is no percentage and no time remaining — an indeterminate bar beside a
/// throughput figure is what an unindexed volume honestly supports, and a determinate bar sitting
/// at 0% would read as a stalled transfer rather than as an unmeasured one.
/// </remarks>
public sealed partial class TransferProgressViewModel : ObservableObject
{
    private readonly TransferRate _rate = new();
    private readonly Stopwatch _elapsed = Stopwatch.StartNew();
    private readonly Action? _cancel;

    public TransferProgressViewModel(TransferPlan plan, TransferEstimate estimate, Action? cancel = null)
    {
        _cancel = cancel;
        Estimate = estimate;
        Verbing = plan.Verb == TransferVerb.Move ? "Moving" : "Copying";
        ItemsTotal = plan.Transfers.Count;
        _bytesTotal = estimate.Bytes;
        _isIndeterminate = !estimate.IsUsable;

        foreach (var transfer in plan.Transfers)
            Items.Add(new TransferItemRow(plan, transfer));

        _headline = $"{Verbing} {ItemsTotal:N0} item(s)…";
    }

    /// <summary>The plan's byte total and whether it can be trusted. Held so the rate calculator
    /// can refuse to produce a time remaining off a figure that is only a floor.</summary>
    public TransferEstimate Estimate { get; private set; }

    public ObservableCollection<TransferItemRow> Items { get; } = [];

    public string Verbing { get; }

    public int ItemsTotal { get; }

    [ObservableProperty]
    private string _headline;

    [ObservableProperty]
    private string _currentName = "";

    [ObservableProperty]
    private int _itemsDone;

    [ObservableProperty]
    private long _bytesDone;

    [ObservableProperty]
    private long _bytesTotal;

    /// <summary>0 to 1 for the bar. Meaningless — and not shown — while
    /// <see cref="IsIndeterminate"/> holds.</summary>
    [ObservableProperty]
    private double _fraction;

    [ObservableProperty]
    private bool _isIndeterminate;

    /// <summary>e.g. "4.2 GB of 50 GB", or just "4.2 GB" when there is no trustworthy total.</summary>
    [ObservableProperty]
    private string _bytesText = "";

    [ObservableProperty]
    private string _rateText = "";

    [ObservableProperty]
    private string _etaText = "";

    /// <summary>The three figures above joined for the status bar, skipping any that has nothing
    /// to say — so a transfer with no trustworthy total shows "4.2 GB · 112 MB/s" rather than
    /// carrying empty gaps and stray separators.</summary>
    [ObservableProperty]
    private string _detailText = "";

    /// <summary>True once the user has asked to stop, so both surfaces can say so and refuse a
    /// second press — a cancel takes effect at the next chunk boundary, not instantly.</summary>
    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CancelCommand))]
    private bool _isCancelling;

    /// <summary>True while the run is held at a chunk boundary. Set by the queue, which owns the
    /// gate; this surface only has to say so and stop its clock.</summary>
    [ObservableProperty]
    private bool _isPaused;

    /// <summary>What the headline said before "Paused — " was put in front of it.</summary>
    private string? _headlineBeforePause;

    private bool CanCancel => _cancel is not null && !IsCancelling;

    [RelayCommand(CanExecute = nameof(CanCancel))]
    private void Cancel()
    {
        IsCancelling = true;
        Headline = "Stopping…";
        _headlineBeforePause = null;
        _cancel?.Invoke();
    }

    /// <summary>
    /// Holds or releases the clock and the wording. Called by the queue on both sides of a pause.
    /// </summary>
    /// <remarks>
    /// <b>The stopwatch stops.</b> <see cref="TransferRate"/> is fed elapsed time rather than
    /// reading a clock, so a stopwatch left running across a five-minute pause hands the first
    /// sample after the resume a huge interval with almost no bytes in it — and the smoothing then
    /// carries that invented stall into the throughput figure for several seconds. Stopping the
    /// watch means the rate simply picks up where it left off, which is what actually happened.
    /// </remarks>
    internal void SetPaused(bool paused)
    {
        if (paused == IsPaused || IsCancelling) return;

        if (paused)
        {
            _elapsed.Stop();
            _headlineBeforePause = Headline;
            Headline = $"Paused — {Headline}";

            // Nothing is moving, so there is no throughput and no time remaining. Leaving "112 MB/s
            // · about 7 minutes left" beside the word "Paused" states two things that contradict
            // each other, and the byte count is the only one of the three still true.
            RateText = "";
            EtaText = "";
            DetailText = JoinDetails();
        }
        else
        {
            _elapsed.Start();
            if (_headlineBeforePause is { } previous) Headline = previous;
            _headlineBeforePause = null;
        }

        IsPaused = paused;
    }

    /// <summary>
    /// Takes one report from the executor. Must run on the UI thread, which is what constructing
    /// the <see cref="System.Progress{T}"/> there arranges.
    /// </summary>
    public void Apply(TransferProgress progress)
    {
        ItemsDone = progress.Done;
        CurrentName = progress.CurrentName;
        BytesDone = progress.BytesDone;

        _rate.Observe(progress.BytesDone, _elapsed.Elapsed);

        BytesText = DescribeBytes();
        if (!IsIndeterminate && BytesTotal > 0)
            Fraction = Math.Clamp(progress.BytesDone / (double)BytesTotal, 0d, 1d);

        RateText = RateFormatter.Speed(_rate.BytesPerSecond);
        EtaText = RateFormatter.Remaining(_rate.Remaining(progress.BytesDone, Estimate));
        DetailText = JoinDetails();

        // A report already on the dispatcher can land after the pause took hold; it must not
        // overwrite "Paused — …" with the wording of a run that is no longer moving.
        if (!IsCancelling && !IsPaused)
            Headline = progress.CurrentName.Length > 0
                ? $"{Verbing} {Math.Min(progress.Done + 1, ItemsTotal):N0} of {ItemsTotal:N0} — {progress.CurrentName}"
                : $"{Verbing} {ItemsTotal:N0} item(s)…";

        MarkRows(progress.Done, progress.CurrentName);
    }

    /// <summary>
    /// The estimate can turn out to be a floor: a move predicted as a rename that crosses a mount
    /// point really does copy its bytes. Growing the total is the honest answer, and it is what
    /// keeps <see cref="Fraction"/> from pinning at 100% for the rest of the transfer.
    /// </summary>
    private string DescribeBytes()
    {
        if (!IsIndeterminate && BytesDone > BytesTotal)
        {
            BytesTotal = BytesDone;
            Estimate = Estimate with { Bytes = BytesDone };
        }

        var done = ByteSizeFormatter.Format(BytesDone);
        return IsIndeterminate || BytesTotal <= 0
            ? done
            : $"{done} of {ByteSizeFormatter.Format(BytesTotal)}";
    }

    private string JoinDetails() =>
        string.Join("  ·  ", new[] { BytesText, RateText, EtaText }.Where(part => part.Length > 0));

    /// <summary>
    /// Rows only ever advance Waiting → Working → Done, in order, so only the ones between the last
    /// report and this one can have changed.
    /// </summary>
    /// <remarks>
    /// It used to rewrite every row on every report. That was invisible for a drop of forty items
    /// and quadratic for a merged folder of five thousand, at ten reports a second — the same shape
    /// of mistake as the thumbnail panel measuring every tile for every arriving image.
    /// </remarks>
    private void MarkRows(int done, string currentName)
    {
        var working = currentName.Length > 0 ? done : -1;
        var last = Math.Min(Math.Max(done, working), Items.Count - 1);

        for (var i = _markedUpTo; i <= last; i++)
            Items[i].State = i < done
                ? TransferItemState.Done
                : i == working
                    ? TransferItemState.Working
                    : TransferItemState.Waiting;

        // The row in flight is not settled yet, so it is re-examined on the next report.
        _markedUpTo = Math.Max(0, Math.Min(done, Items.Count - 1));
    }

    private int _markedUpTo;

    /// <summary>
    /// A fixed, plausible mid-transfer state, so the two surfaces can be photographed without a
    /// real transfer running — which would be neither reproducible nor safe to hold still.
    /// </summary>
    internal void PoseForCapture(int itemsDone, long bytesDone, double bytesPerSecond)
    {
        ItemsDone = itemsDone;
        CurrentName = Items.ElementAtOrDefault(itemsDone)?.Name ?? "";
        BytesDone = bytesDone;
        BytesText = DescribeBytes();
        if (!IsIndeterminate && BytesTotal > 0)
            Fraction = Math.Clamp(bytesDone / (double)BytesTotal, 0d, 1d);

        RateText = RateFormatter.Speed(bytesPerSecond);
        EtaText = RateFormatter.Remaining(bytesPerSecond > 0 && !IsIndeterminate
            ? TimeSpan.FromSeconds((BytesTotal - bytesDone) / bytesPerSecond)
            : null);
        DetailText = JoinDetails();
        Headline = $"{Verbing} {Math.Min(itemsDone + 1, ItemsTotal):N0} of {ItemsTotal:N0} — {CurrentName}";
        MarkRows(itemsDone, CurrentName);
    }
}
