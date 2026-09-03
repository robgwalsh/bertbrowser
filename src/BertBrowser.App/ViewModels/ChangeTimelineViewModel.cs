using System.Windows;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.Core.Data;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Changes;
using BertBrowser.Core.Services.Mft;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Data.Sqlite;

namespace BertBrowser.App.ViewModels;

/// <summary>One choice on the range list.</summary>
public sealed record ChangeRangeOption(ChangeRange Range, string Label);

/// <summary>
/// "What changed here in the last hour?", answered from the change log the index helper's USN
/// tail writes. Nothing here reads the journal: the window reads rows, and the rows were written
/// as the changes happened.
/// </summary>
/// <remarks>
/// <para>
/// Live, because the question is usually asked while the answer is still arriving: an installer
/// is running, a build is writing. A timer polls the log's <see cref="ChangeLogRepository.Stamp"/>
/// every couple of seconds and re-queries only when it moved, so an idle machine costs one tiny
/// read per tick and nothing on screen changes.
/// </para>
/// <para>
/// The banner is where the four ways this cannot answer are told apart. Recording off is the one
/// that matters most: it is the default, and a window that showed an empty list for it would read
/// as "nothing changed" — the opposite of the truth.
/// </para>
/// </remarks>
public sealed partial class ChangeTimelineViewModel : ObservableObject, IDisposable
{
    private static readonly TimeSpan LivePollInterval = TimeSpan.FromSeconds(2);

    /// <summary>A relative range slides: "the last hour" drops rows as they age out, which the
    /// stamp cannot see. So a reload happens on its own this often even when nothing was written.</summary>
    private const int TicksBetweenForcedReloads = 15;

    private readonly ChangeLogRepository _log;
    private readonly IMftIndexService _mftIndex;
    private readonly AppSettings _settings;
    private readonly DispatcherTimer _timer;

    private CancellationTokenSource? _cts;
    private ChangeLogStamp _lastStamp;
    private int _ticksSinceReload;
    private bool _probing;
    private bool _disposed;

    public ChangeTimelineViewModel(ChangeLogRepository log, IMftIndexService mftIndex, AppSettings settings)
    {
        _log = log;
        _mftIndex = mftIndex;
        _settings = settings;

        _markUtc = settings.ChangeTimelineMarkUtc;
        _includeHidden = settings.ShowHiddenItems;

        _mftIndex.IndexRefreshed += OnIndexRefreshed;
        _mftIndex.StatusChanged += OnStatusChanged;

        _timer = new DispatcherTimer(DispatcherPriority.Background) { Interval = LivePollInterval };
        _timer.Tick += OnTick;
        _timer.Start();
    }

    public IReadOnlyList<ChangeRangeOption> RangeOptions { get; } =
    [
        new(ChangeRange.Last15Minutes, "Last 15 minutes"),
        new(ChangeRange.LastHour, "Last hour"),
        new(ChangeRange.Last6Hours, "Last 6 hours"),
        new(ChangeRange.Last24Hours, "Last 24 hours"),
        new(ChangeRange.SinceMark, "Since the mark"),
    ];

    /// <summary>The folder being watched, or null for every indexed drive.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ScopeDisplay))]
    private string? _scopePath;

    public string ScopeDisplay => ScopePath is { Length: > 0 } path ? path : "This PC";

    [ObservableProperty]
    private ChangeRange _range = ChangeRange.LastHour;

    [ObservableProperty] private bool _showCreated = true;
    [ObservableProperty] private bool _showModified = true;
    [ObservableProperty] private bool _showDeleted = true;
    [ObservableProperty] private bool _showRenamed = true;
    [ObservableProperty] private bool _includeHidden;

    partial void OnRangeChanged(ChangeRange value) => Reload();
    partial void OnShowCreatedChanged(bool value) => Reload();
    partial void OnShowModifiedChanged(bool value) => Reload();
    partial void OnShowDeletedChanged(bool value) => Reload();
    partial void OnShowRenamedChanged(bool value) => Reload();
    partial void OnIncludeHiddenChanged(bool value) => Reload();

    /// <summary>When "Mark now" was last pressed. Kept in settings, so it survives a reboot an
    /// installer asked for.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(MarkText))]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    private DateTime? _markUtc;

    public string MarkText => MarkUtc is { } mark ? $"Marked at {mark.ToLocalTime():t}" : "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    [NotifyPropertyChangedFor(nameof(SummaryText))]
    private IReadOnlyList<ChangeEventViewModel> _rows = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    private bool _isLoading;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasMessage))]
    [NotifyPropertyChangedFor(nameof(Message))]
    [NotifyPropertyChangedFor(nameof(CanRetry))]
    [NotifyPropertyChangedFor(nameof(CanOpenSettings))]
    [NotifyPropertyChangedFor(nameof(EmptyMessage))]
    private ChangeTimelineAvailability _availability = ChangeTimelineAvailability.Ready;

    /// <summary>The indexer's own words, relayed verbatim for the reason the disk-usage view does.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Message))]
    private string _indexStatus = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SummaryText))]
    private bool _truncated;

    public bool HasMessage => Availability != ChangeTimelineAvailability.Ready;

    public string Message => Availability switch
    {
        ChangeTimelineAvailability.RecordingOff =>
            "File changes aren't being recorded. Turn recording on under Settings › History to see what changes here.",
        ChangeTimelineAvailability.Building => IndexStatus is { Length: > 0 } status
            ? $"{status} Recording starts once the index is built."
            : "Still indexing — recording starts once the index is built.",
        ChangeTimelineAvailability.IndexerUnavailable => IndexStatus is { Length: > 0 } status
            ? $"{status} Nothing is being recorded."
            : "The search index isn't running, so nothing is being recorded.",
        ChangeTimelineAvailability.ScopeNotIndexed =>
            "This drive isn't indexed, so changes on it aren't recorded.",
        _ => "",
    };

    /// <summary>A retry is a UAC prompt, so it is a button and never a timer.</summary>
    public bool CanRetry => Availability == ChangeTimelineAvailability.IndexerUnavailable && _mftIndex.CanRetry;

    public bool CanOpenSettings => Availability == ChangeTimelineAvailability.RecordingOff;

    /// <summary>Why the list is empty, when it is — an unexplained blank list reads as a failure.</summary>
    public string EmptyMessage
    {
        get
        {
            if (IsLoading || Rows.Count > 0 || Availability == ChangeTimelineAvailability.RecordingOff) return "";
            if (Range == ChangeRange.SinceMark && MarkUtc is null)
                return "Press \"Mark now\", then do the thing you want to watch.";
            return ChangeLogRules.EmptyMessage(Range, ScopePath is { Length: > 0 });
        }
    }

    public string SummaryText => (Rows.Count, Truncated) switch
    {
        (0, _) => "",
        (_, true) => $"Showing the newest {Rows.Count:N0} changes.",
        (1, _) => "1 change",
        var (n, _) => $"{n:N0} changes",
    };

    [RelayCommand]
    private Task Refresh() => LoadAsync(ScopePath);

    /// <summary>The installer case: press this, run the thing, and the list shows only what came after.</summary>
    [RelayCommand]
    private void MarkNow()
    {
        MarkUtc = DateTime.UtcNow;
        _settings.ChangeTimelineMarkUtc = MarkUtc;
        _settings.Save();

        if (Range == ChangeRange.SinceMark)
            Reload();
        else
            Range = ChangeRange.SinceMark; // reloads through OnRangeChanged
    }

    [RelayCommand]
    private void Retry()
    {
        _mftIndex.Retry();
        OnPropertyChanged(nameof(CanRetry));
    }

    /// <summary>Puts the selected rows on the clipboard, one per line, tab-separated.</summary>
    public bool CopyRows(IEnumerable<ChangeEventViewModel> rows)
    {
        var text = string.Join(Environment.NewLine, rows.Select(r => r.CopyLine));
        return text.Length > 0 && FileClipboard.TrySetText(text);
    }

    private void Reload() => _ = LoadAsync(ScopePath, silent: true);

    /// <summary>
    /// Points the view at <paramref name="scopePath"/> (null being "This PC") and re-queries.
    /// </summary>
    /// <param name="silent">A live refresh: no progress bar, so the window does not flicker every
    /// couple of seconds while an installer runs.</param>
    public async Task LoadAsync(string? scopePath, bool silent = false)
    {
        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;
        var ct = cts.Token;

        ScopePath = scopePath;
        if (!silent) IsLoading = true;
        IndexStatus = _mftIndex.StatusText;
        _ticksSinceReload = 0;

        try
        {
            var availability = ComputeAvailability(scopePath);
            var now = DateTime.UtcNow;
            var since = ChangeLogRules.SinceUtc(Range, now, MarkUtc, _settings.EffectiveChangeLogPolicy());

            // Off: nothing is written, and what was is being wiped. Querying would only flash
            // rows on their way out. No mark: nothing to count from yet — but the stamp is still
            // taken, or every tick would see it "moved" and come back here.
            if (availability == ChangeTimelineAvailability.RecordingOff || since is null)
            {
                if (since is null && availability != ChangeTimelineAvailability.RecordingOff)
                    _lastStamp = await Task.Run(_log.Stamp, ct);
                Rows = [];
                Truncated = false;
                Availability = availability;
                return;
            }

            var scopeKey = scopePath is { Length: > 0 } ? PathKey.Canonicalize(scopePath) : null;
            var query = new ChangeQuery(since.Value, scopeKey, Kinds(), IncludeHidden, ChangeLogRules.QueryLimit);

            // The stamp is read before the query, so a write that lands between the two moves it
            // and the next tick reloads; the other order would miss that write until the one after.
            var (stamp, result) = await Task.Run(() => (_log.Stamp(), _log.Query(query)), ct);
            ct.ThrowIfCancellationRequested();

            _lastStamp = stamp;
            Rows = result.Rows.Select(r => new ChangeEventViewModel(r, now)).ToList();
            Truncated = result.Truncated;
            Availability = availability;
        }
        catch (OperationCanceledException)
        {
            // Superseded by a newer load; the newer call owns the view now.
        }
        catch (SqliteException)
        {
            // A database the helper is mid-write on, or one an older helper never created the
            // table in. The next tick tries again; the rows on screen stay as they were.
        }
        finally
        {
            if (ReferenceEquals(_cts, cts))
                IsLoading = false;
        }
    }

    private ChangeTimelineAvailability ComputeAvailability(string? scopePath)
    {
        var scoped = scopePath is { Length: > 0 };
        var scopeIndexed = scoped && _mftIndex.IsIndexed(PathKey.Canonicalize(scopePath!));
        return ChangeLogRules.Availability(
            recordingOn: _settings.RecordFileChanges,
            scoped: scoped,
            anyIndexed: _mftIndex.AnyIndexed,
            scopeIndexed: scopeIndexed,
            isBuilding: _mftIndex.IsBuilding,
            indexerRunning: !_mftIndex.CanRetry);
    }

    private IReadOnlySet<ChangeKind> Kinds()
    {
        var kinds = new HashSet<ChangeKind>();
        if (ShowCreated) kinds.Add(ChangeKind.Created);
        if (ShowModified) kinds.Add(ChangeKind.Modified);
        if (ShowDeleted) kinds.Add(ChangeKind.Deleted);
        if (ShowRenamed) kinds.Add(ChangeKind.Renamed);
        return kinds;
    }

    /// <summary>
    /// The live half. Ages the "3 min ago" labels every tick; reloads when the log's stamp moved,
    /// when the setting flipped under us (the settings dialog is modal, this window is not), and
    /// every so often regardless so a sliding range slides.
    /// </summary>
    private async void OnTick(object? sender, EventArgs e)
    {
        if (_disposed || _probing) return;

        _probing = true;
        try
        {
            var now = DateTime.UtcNow;
            foreach (var row in Rows)
                row.Touch(now);

            var recordingOn = _settings.RecordFileChanges;
            var showingOff = Availability == ChangeTimelineAvailability.RecordingOff;
            if (recordingOn == showingOff)
            {
                await LoadAsync(ScopePath, silent: true);
                return;
            }
            if (!recordingOn) return;

            var stamp = await Task.Run(_log.Stamp);
            if (_disposed) return;

            if (stamp != _lastStamp || ++_ticksSinceReload >= TicksBetweenForcedReloads)
                await LoadAsync(ScopePath, silent: true);
        }
        catch (Exception ex) when (ex is SqliteException or IOException or UnauthorizedAccessException or ArgumentException)
        {
            // A database mid-write, or a tab path the canonicaliser refuses. This is an async
            // void on a timer that fires for the life of a modeless window; an escape here is
            // the process, not the window. The next tick tries again.
        }
        finally
        {
            _probing = false;
        }
    }

    /// <summary>A volume finishing its build is when recording on it starts, so the banner has to
    /// move on from "still indexing" without the window being reopened.</summary>
    private void OnIndexRefreshed(string rootKey)
    {
        if (_disposed) return;
        Post(() => _ = LoadAsync(ScopePath, silent: true));
    }

    private void OnStatusChanged()
    {
        if (_disposed) return;
        Post(() =>
        {
            IndexStatus = _mftIndex.StatusText;
            Availability = ComputeAvailability(ScopePath);
        });
    }

    /// <summary>Both index events arrive on a worker thread.</summary>
    private static void Post(Action action) =>
        Application.Current?.Dispatcher.InvokeAsync(action);

    /// <summary>Modeless window, long-lived services: a missed detach pins this view model — and
    /// its timer — for the rest of the session.</summary>
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        _timer.Stop();
        _timer.Tick -= OnTick;
        _mftIndex.IndexRefreshed -= OnIndexRefreshed;
        _mftIndex.StatusChanged -= OnStatusChanged;

        _cts?.Cancel();
        _cts?.Dispose();
        _cts = null;
    }
}
