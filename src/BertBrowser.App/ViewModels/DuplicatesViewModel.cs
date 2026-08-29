using System.Collections.ObjectModel;
using System.Windows;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Duplicates;
using BertBrowser.Core.Services.Mft;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// Removes these copies through the app's ordinary reversible delete, and answers with the ones
/// that really went.
/// </summary>
/// <remarks>
/// Supplied rather than reached for, the way <c>DiskUsageWindow</c> takes its reveal: it keeps this
/// view model from knowing about the shell, and it keeps every delete in the app going through the
/// one plan/confirm/execute chain that owns the Recycle Bin and the undo slot.
/// </remarks>
public delegate Task<IReadOnlyCollection<string>> CopyRemover(IReadOnlyList<string> paths);

/// <summary>
/// "What do I have two of?", answered by shortlisting on the byte lengths the MFT pass already
/// wrote and then reading only the files that collide.
/// </summary>
/// <remarks>
/// <para>
/// Unlike the disk-usage view this one really does touch the disk, so it is the only analysis
/// screen here with a cancel. Everything else about it follows that view: one request in flight,
/// abandoned when the root changes; an availability banner rather than a silently empty list; and
/// a re-query when a volume finishes indexing.
/// </para>
/// <para>
/// It never runs off a keystroke. A whole-PC scan is a full pass over the index followed by real
/// reads, so it is reached from an explicit gesture and says how far along it is throughout.
/// </para>
/// </remarks>
public sealed partial class DuplicatesViewModel : ObservableObject, IDisposable
{
    private readonly IDuplicateFinder _finder;
    private readonly IMftIndexService _mftIndex;
    private readonly CopyRemover _removeCopies;
    private readonly bool _includeHidden;

    /// <summary>Cancels the scan in flight — both the user's Stop and a re-pointed root.</summary>
    private CancellationTokenSource? _cts;
    private bool _disposed;

    public DuplicatesViewModel(
        IDuplicateFinder finder,
        IMftIndexService mftIndex,
        CopyRemover removeCopies,
        bool includeHidden,
        long minSizeBytes,
        bool skipSystemFolders)
    {
        _finder = finder;
        _mftIndex = mftIndex;
        _removeCopies = removeCopies;
        _includeHidden = includeHidden;
        _minSizeBytes = minSizeBytes;
        _skipSystemFolders = skipSystemFolders;
        MinSizeOptions = MinSizeOption.Including(minSizeBytes);

        _mftIndex.IndexRefreshed += OnIndexRefreshed;
        _mftIndex.StatusChanged += OnStatusChanged;
    }

    /// <summary>The folder being searched, or null for "This PC".</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RootDisplay))]
    private string? _rootPath;

    public string RootDisplay => RootPath is { Length: > 0 } path ? path : "This PC";

    // --- knobs ---

    /// <summary>
    /// The floor below which files are not considered.
    /// </summary>
    /// <remarks>
    /// Not cosmetic. It bounds both the shortlist's memory and how much has to be read, and
    /// duplicate 400-byte files are not what anyone opens this for.
    /// </remarks>
    [ObservableProperty]
    private long _minSizeBytes;

    [ObservableProperty]
    private bool _skipSystemFolders;

    /// <summary>
    /// The picker's entries — the shipped magnitudes, plus whatever the session actually started
    /// from if that is not one of them.
    /// </summary>
    /// <remarks>
    /// The same trap the aspect-ratio picker documents: a list of presets alone shows an empty box
    /// for any other value, and then quietly rewrites it to whatever the user picks next. Settings
    /// is a hand-editable JSON file, and the harness runs at a byte.
    /// </remarks>
    public IReadOnlyList<MinSizeOption> MinSizeOptions { get; }

    // --- state ---

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    [NotifyCanExecuteChangedFor(nameof(ScanCommand))]
    [NotifyCanExecuteChangedFor(nameof(CancelCommand))]
    private bool _isScanning;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasMessage))]
    [NotifyPropertyChangedFor(nameof(Message))]
    [NotifyPropertyChangedFor(nameof(CanRetry))]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    private DuplicateScanAvailability _availability = DuplicateScanAvailability.Ready;

    /// <summary>The indexer's own words, relayed verbatim so this and the status bar cannot
    /// describe the same state differently.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Message))]
    private string _indexStatus = "";

    /// <summary>True when the scan was stopped or could not read something, so the list is a
    /// floor rather than the answer.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasCaveat))]
    [NotifyPropertyChangedFor(nameof(Caveat))]
    private bool _wasCancelled;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasCaveat))]
    [NotifyPropertyChangedFor(nameof(Caveat))]
    private bool _wasIncomplete;

    /// <summary>True once a scan has run, so an untouched window says "press Scan" rather than
    /// "nothing found".</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    private bool _hasScanned;

    public ObservableCollection<DuplicateGroupViewModel> Groups { get; } = [];

    // --- progress ---

    [ObservableProperty]
    private string _progressHeadline = "";

    [ObservableProperty]
    private string _progressDetail = "";

    [ObservableProperty]
    private double _progressFraction;

    /// <summary>
    /// True while the shortlist is being read. Counting the index's rows first would cost a second
    /// full scan of it, so there is no honest denominator here — and a determinate bar pinned at
    /// zero reads as a stall rather than as work that cannot be measured.
    /// </summary>
    [ObservableProperty]
    private bool _isProgressIndeterminate = true;

    // --- totals ---

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SummaryText))]
    private long _reclaimableBytes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SummaryText))]
    [NotifyCanExecuteChangedFor(nameof(RemoveTickedCommand))]
    private int _tickedCount;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SummaryText))]
    [NotifyCanExecuteChangedFor(nameof(RemoveTickedCommand))]
    private long _tickedBytes;

    public string SummaryText => Groups.Count == 0
        ? ""
        : TickedCount > 0
            ? $"{Groups.Count} groups · {ByteSizeFormatter.Format(ReclaimableBytes)} reclaimable · " +
              $"{TickedCount} selected ({ByteSizeFormatter.Format(TickedBytes)})"
            : $"{Groups.Count} groups · {ByteSizeFormatter.Format(ReclaimableBytes)} reclaimable";

    public bool HasMessage => Availability != DuplicateScanAvailability.Ready;

    /// <summary>
    /// What to say when the answer cannot be believed. Each of these is a different thing having
    /// gone wrong, and saying which is the whole reason
    /// <see cref="DuplicateScanAvailability"/> exists — a single "no duplicates" would leave an
    /// unindexed drive looking like a tidy one.
    /// </summary>
    public string Message => Availability switch
    {
        DuplicateScanAvailability.Building => IndexStatus is { Length: > 0 } status
            ? $"{status} — anything found so far is only part of the answer."
            : "Still indexing — anything found so far is only part of the answer.",

        DuplicateScanAvailability.NoSizeData =>
            "This drive is indexed by name only. Without file sizes there is nothing to compare, " +
            "so duplicates cannot be found here.",

        DuplicateScanAvailability.NotIndexed =>
            "This drive isn't indexed, and finding duplicates starts from the index rather than " +
            "from reading every folder.",

        _ => "",
    };

    public bool HasCaveat => WasCancelled || WasIncomplete;

    /// <summary>Said separately from <see cref="Message"/>: the answer is usable, just short.</summary>
    public string Caveat => (WasCancelled, WasIncomplete) switch
    {
        (true, true) => "Stopped early, and some files could not be read — there may be more.",
        (true, false) => "Stopped early — there may be more.",
        (false, true) => "Some files could not be read, so there may be more.",
        _ => "",
    };

    /// <summary>An unexplained blank panel reads as a failure; saying why reads as the fact it is.</summary>
    public string EmptyMessage => (IsScanning, Groups.Count, HasScanned, Availability) switch
    {
        (true, _, _, _) => "",
        (_, > 0, _, _) => "",
        (_, _, false, _) => "Press Scan to look for identical files.",
        (_, _, _, DuplicateScanAvailability.Ready) => "No duplicates here.",
        _ => "Needs the search index — see above.",
    };

    /// <summary>Offered only where a retry could change the answer, and only ever as a button:
    /// a retry raises a UAC prompt, so nothing here retries on a timer.</summary>
    public bool CanRetry => Availability == DuplicateScanAvailability.NotIndexed && _mftIndex.CanRetry;

    // --- commands ---

    private bool CanScan => !IsScanning;

    [RelayCommand(CanExecute = nameof(CanScan))]
    private Task Scan() => ScanAsync(RootPath);

    private bool CanCancelScan => IsScanning;

    [RelayCommand(CanExecute = nameof(CanCancelScan))]
    private void Cancel()
    {
        ProgressHeadline = "Stopping…";
        _cts?.Cancel();
    }

    [RelayCommand]
    private void Retry()
    {
        _mftIndex.Retry();
        OnPropertyChanged(nameof(CanRetry));
    }

    [RelayCommand]
    private void KeepNewest() => TickAllBut(KeepStrategy.Newest);

    [RelayCommand]
    private void KeepOldest() => TickAllBut(KeepStrategy.Oldest);

    [RelayCommand]
    private void KeepShallowest() => TickAllBut(KeepStrategy.Shallowest);

    [RelayCommand]
    private void ClearSelection()
    {
        foreach (var group in Groups) group.ClearTicks();
        RecomputeTotals();
    }

    private bool CanRemoveTicked => TickedCount > 0 && !IsScanning;

    /// <summary>
    /// Hands the ticked copies to the app's ordinary delete and drops whatever really went.
    /// </summary>
    /// <remarks>
    /// Every group is re-checked against <see cref="DuplicateRules.CanRemove"/> first, so a group
    /// that somehow arrived with every copy ticked contributes nothing rather than being emptied.
    /// The list is not re-scanned afterwards: the copies still standing were confirmed identical by
    /// this run and have not been touched.
    /// </remarks>
    [RelayCommand(CanExecute = nameof(CanRemoveTicked))]
    private async Task RemoveTickedAsync()
    {
        var paths = new List<string>();
        foreach (var group in Groups)
        {
            if (!DuplicateRules.CanRemove(group.Files.Count, group.TickedCount)) continue;
            paths.AddRange(group.Files.Where(f => f.IsTicked).Select(f => f.FullPath));
        }

        if (paths.Count == 0) return;

        var removed = await _removeCopies(paths);
        if (removed.Count == 0) return;

        var gone = new HashSet<string>(removed, StringComparer.OrdinalIgnoreCase);
        foreach (var group in Groups.ToList())
        {
            if (!group.Remove(gone)) Detach(group);
        }

        RecomputeTotals();
    }

    // --- the scan ---

    /// <summary>Points the view at <paramref name="rootPath"/> (null being "This PC") and runs.</summary>
    public async Task ScanAsync(string? rootPath)
    {
        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;

        RootPath = rootPath;
        IsScanning = true;
        HasScanned = true;
        WasCancelled = false;
        WasIncomplete = false;
        IndexStatus = _mftIndex.StatusText;

        foreach (var group in Groups.ToList()) Detach(group);
        RecomputeTotals();

        ProgressHeadline = "Looking for files of the same size…";
        ProgressDetail = "";
        ProgressFraction = 0;
        IsProgressIndeterminate = true;

        try
        {
            // Constructed here so it captures the UI dispatcher: the scanner reports from its own
            // reading threads, and this is what marshals them across.
            var progress = new Progress<DuplicateScanProgress>(Apply);

            var request = new DuplicateScanRequest(
                rootPath, MinSizeBytes, _includeHidden, SkipSystemFolders);

            var outcome = await _finder.ScanAsync(request, progress, cts.Token);

            // A superseded scan does not own the view any more.
            if (!ReferenceEquals(_cts, cts)) return;

            Availability = outcome.Availability;
            WasCancelled = outcome.Cancelled;
            WasIncomplete = outcome.Incomplete;

            foreach (var group in outcome.Groups) Attach(new DuplicateGroupViewModel(group));
            RecomputeTotals();
        }
        catch (OperationCanceledException)
        {
            if (ReferenceEquals(_cts, cts)) WasCancelled = true;
        }
        finally
        {
            if (ReferenceEquals(_cts, cts))
            {
                IsScanning = false;
                ProgressHeadline = "";
                ProgressDetail = "";
            }
        }
    }

    private void Apply(DuplicateScanProgress progress)
    {
        if (_disposed || !IsScanning) return;

        switch (progress.Phase)
        {
            case DuplicateScanPhase.Shortlisting:
                ProgressHeadline = "Looking for files of the same size…";
                ProgressDetail = "";
                IsProgressIndeterminate = true;
                return;

            case DuplicateScanPhase.Sampling:
                ProgressHeadline = "Comparing the start of each file…";
                break;

            default:
                ProgressHeadline = "Comparing in full…";
                break;
        }

        IsProgressIndeterminate = false;
        ProgressFraction = progress.BytesTotal > 0
            ? Math.Clamp((double)progress.BytesDone / progress.BytesTotal, 0, 1)
            : 0;

        ProgressDetail =
            $"{progress.Done:N0} of {progress.Total:N0} files · " +
            $"{ByteSizeFormatter.Format(progress.BytesDone)} of {ByteSizeFormatter.Format(progress.BytesTotal)}";
    }

    // --- bookkeeping ---

    private void TickAllBut(KeepStrategy strategy)
    {
        foreach (var group in Groups) group.TickAllBut(strategy);
        RecomputeTotals();
    }

    private void Attach(DuplicateGroupViewModel group)
    {
        group.TicksChanged += RecomputeTotals;
        Groups.Add(group);
    }

    private void Detach(DuplicateGroupViewModel group)
    {
        group.TicksChanged -= RecomputeTotals;
        Groups.Remove(group);
    }

    private void RecomputeTotals()
    {
        var reclaimable = 0L;
        var ticked = 0;
        var tickedBytes = 0L;

        foreach (var group in Groups)
        {
            reclaimable += group.WastedBytes;
            ticked += group.TickedCount;
            tickedBytes += group.TickedCount * group.SizeBytes;
        }

        ReclaimableBytes = reclaimable;
        TickedCount = ticked;
        TickedBytes = tickedBytes;
        OnPropertyChanged(nameof(SummaryText));
        OnPropertyChanged(nameof(EmptyMessage));
    }

    /// <summary>
    /// A volume finishing its index turns "not indexed" into an answer, so an idle window re-runs.
    /// A scan already under way is left alone: restarting it would throw away minutes of hashing.
    /// </summary>
    private void OnIndexRefreshed(string rootKey)
    {
        if (_disposed || IsScanning || !HasScanned) return;
        if (Availability is DuplicateScanAvailability.Ready) return;

        var current = RootPath is { Length: > 0 } path ? PathKey.Canonicalize(path) : null;
        if (current is not null &&
            !current.Equals(rootKey, StringComparison.Ordinal) &&
            !current.StartsWith(rootKey, StringComparison.Ordinal))
            return;

        Post(() => _ = ScanAsync(RootPath));
    }

    private void OnStatusChanged()
    {
        if (_disposed) return;
        Post(() =>
        {
            IndexStatus = _mftIndex.StatusText;
            OnPropertyChanged(nameof(CanRetry));
        });
    }

    /// <summary>Both index events arrive on a worker thread.</summary>
    private static void Post(Action action) =>
        Application.Current?.Dispatcher.InvokeAsync(action);

    /// <summary>
    /// Unsubscribing matters more here than in most places: this window is modeless and outlives
    /// index completions, so a missed detach pins the view model for the rest of the session.
    /// </summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        _mftIndex.IndexRefreshed -= OnIndexRefreshed;
        _mftIndex.StatusChanged -= OnStatusChanged;

        foreach (var group in Groups) group.TicksChanged -= RecomputeTotals;

        _cts?.Cancel();
        _cts?.Dispose();
        _cts = null;
    }
}

/// <summary>One entry in the minimum-size picker.</summary>
/// <remarks>
/// A fixed list rather than a free number: the choice that matters is an order of magnitude, and
/// every value here is one somebody would actually pick.
/// </remarks>
public sealed record MinSizeOption(string Label, long Bytes)
{
    public static IReadOnlyList<MinSizeOption> All { get; } =
    [
        new("1 KB", 1024),
        new("100 KB", 100 * 1024),
        new("1 MB", 1024 * 1024),
        new("10 MB", 10L * 1024 * 1024),
        new("100 MB", 100L * 1024 * 1024),
        new("1 GB", 1024L * 1024 * 1024),
    ];

    /// <summary><see cref="All"/>, plus <paramref name="bytes"/> when it is not already one of them,
    /// so a value from settings or the command line is shown rather than silently discarded.</summary>
    public static IReadOnlyList<MinSizeOption> Including(long bytes)
    {
        if (All.Any(o => o.Bytes == bytes)) return All;

        return [.. All.Append(new MinSizeOption(ByteSizeFormatter.Format(bytes), bytes))
            .OrderBy(o => o.Bytes)];
    }

    public override string ToString() => Label;
}
