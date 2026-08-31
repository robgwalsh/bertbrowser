using System.Collections.ObjectModel;
using System.Windows;
using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Services.Mft;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// "What is taking up my disk?", answered entirely from what the MFT pass already wrote — the
/// per-file rows in <c>fs_entry</c> and the per-directory totals in <c>dir_size_cache</c>. Nothing
/// here walks the filesystem to size a folder.
/// </summary>
/// <remarks>
/// <para>
/// Two views over the same root: the biggest files anywhere beneath it, and what its immediate
/// children weigh. The second is drillable, which is how you follow the weight down to whatever is
/// actually responsible for it.
/// </para>
/// <para>
/// Unlike a search box, this never runs off a keystroke. A whole-PC query is a full scan of the
/// index and takes seconds on a large disk, so it is deliberately gated behind an explicit gesture,
/// runs off-thread, and is abandoned the moment the root changes.
/// </para>
/// </remarks>
public sealed partial class DiskUsageViewModel : ObservableObject, IDisposable
{
    /// <summary>Enough to find what is eating a disk; far past what anyone scrolls.</summary>
    private const int LargestFilesLimit = 500;

    private readonly IDiskUsageService _service;
    private readonly IMftIndexService _mftIndex;
    private readonly bool _includeHidden;

    /// <summary>Cancels the query in flight. Re-navigating abandons the previous one rather than
    /// letting two results race to land — the same per-request pattern a tab uses.</summary>
    private CancellationTokenSource? _cts;
    private bool _disposed;

    public DiskUsageViewModel(IDiskUsageService service, IMftIndexService mftIndex, bool includeHidden)
    {
        _service = service;
        _mftIndex = mftIndex;
        _includeHidden = includeHidden;

        _mftIndex.IndexRefreshed += OnIndexRefreshed;
        _mftIndex.StatusChanged += OnStatusChanged;
    }

    /// <summary>The folder being examined, or null for "This PC".</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RootDisplay))]
    [NotifyPropertyChangedFor(nameof(CanGoUp))]
    private string? _rootPath;

    public string RootDisplay => RootPath is { Length: > 0 } path ? path : "This PC";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(LargestFilesEmptyMessage))]
    private bool _isLoading;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasMessage))]
    [NotifyPropertyChangedFor(nameof(Message))]
    [NotifyPropertyChangedFor(nameof(CanRetry))]
    [NotifyPropertyChangedFor(nameof(LargestFilesEmptyMessage))]
    private DiskUsageAvailability _availability = DiskUsageAvailability.Ready;

    /// <summary>The indexer's own words, relayed verbatim rather than paraphrased, so the banner
    /// here and the status bar can never describe the same state differently.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Message))]
    private string _indexStatus = "";

    public ObservableCollection<FileItemViewModel> LargestFiles { get; } = [];
    public ObservableCollection<DiskUsageTileViewModel> Children { get; } = [];

    /// <summary>The folder's own total, and the part of it the children do not explain.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(TotalText))]
    private long? _totalBytes;

    public string TotalText => TotalBytes is { } bytes ? ByteSizeFormatter.Format(bytes) : "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasUnknownChildren))]
    private int _unknownChildCount;

    public bool HasUnknownChildren => UnknownChildCount > 0;

    public bool CanGoUp => RootPath is { Length: > 0 } path && Path.GetDirectoryName(path) is not null;

    public bool HasMessage => Availability != DiskUsageAvailability.Ready;

    /// <summary>
    /// What to say when the numbers cannot be shown. Each of these is a different thing having gone
    /// wrong, and saying so is the whole reason <see cref="DiskUsageAvailability"/> exists — a
    /// single "no data" would leave an unindexed drive looking like an empty one.
    /// </summary>
    public string Message => Availability switch
    {
        DiskUsageAvailability.Building => IndexStatus is { Length: > 0 } status
            ? $"{status}"
            : "Still indexing",

        // Both of these are careful to say what is missing rather than "no sizes": the files listed
        // beside the banner have real sizes, because those come from reading the folder rather than
        // from the index. It is the folder totals, and the search for large files below here, that
        // the index is needed for.
        DiskUsageAvailability.NoSizeData =>
            "This drive is indexed by name only — nothing measured it, so folder totals aren't available.",
        DiskUsageAvailability.NotIndexed =>
            "This drive isn't indexed, so folder totals aren't available. File sizes here are real.",

        DiskUsageAvailability.NotAPath => "That isn't a folder this can measure.",
        _ => "",
    };

    /// <summary>Why the largest-files panel is empty, when it is. An unexplained blank panel reads
    /// as a failure; "nothing indexed here" reads as the fact it is.</summary>
    public string LargestFilesEmptyMessage => (IsLoading, LargestFiles.Count, Availability) switch
    {
        (true, _, _) => "",
        (_, > 0, _) => "",
        (_, _, DiskUsageAvailability.Ready) => "Nothing below this folder.",
        _ => "Needs the search index — see above.",
    };

    /// <summary>Offered only where a retry could change the answer, and only ever as a button the
    /// user presses: a retry raises a UAC prompt, so nothing here retries on a timer.</summary>
    public bool CanRetry => Availability == DiskUsageAvailability.NotIndexed && _mftIndex.CanRetry;

    [RelayCommand]
    private Task Refresh() => LoadAsync(RootPath);

    [RelayCommand]
    private Task GoUp() =>
        LoadAsync(RootPath is { Length: > 0 } path ? Path.GetDirectoryName(path) : null);

    [RelayCommand]
    private Task DrillInto(DiskUsageTileViewModel? tile) =>
        tile is { IsDirectory: true, IsSynthetic: false } ? LoadAsync(tile.FullPath) : Task.CompletedTask;

    [RelayCommand]
    private void Retry()
    {
        _mftIndex.Retry();
        OnPropertyChanged(nameof(CanRetry));
    }

    /// <summary>
    /// Points the view at <paramref name="rootPath"/> (null being "This PC") and runs both queries.
    /// </summary>
    /// <summary>
    /// Somewhere with contents to break down. Deliberately not just <c>Directory.Exists</c>: the
    /// same widening the navigation gate needed, and for the same reason — a path inside an archive
    /// is a real place with real children that no filesystem call will admit to.
    /// </summary>
    private static bool HasChildren(string path) =>
        Directory.Exists(path) ||
        BertBrowser.Core.Services.Archives.ArchivePath.Parse(path, File.Exists) is not null;

    public async Task LoadAsync(string? rootPath)
    {
        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;
        var ct = cts.Token;

        RootPath = rootPath;
        IsLoading = true;
        IndexStatus = _mftIndex.StatusText;

        try
        {
            var files = await _service.LargestFilesAsync(rootPath, LargestFilesLimit, _includeHidden, ct);
            ct.ThrowIfCancellationRequested();

            LargestFiles.Clear();
            foreach (var hit in files.Files)
                LargestFiles.Add(FileListViewModel.CreateSearchItem(hit));

            var availability = files.Availability;

            // "This PC" has no single parent folder to break down, so the composition half only
            // applies to somewhere that has children — a real directory, or a folder inside a
            // container, where every size is exact and this view is at its best.
            if (rootPath is { Length: > 0 } directory && HasChildren(directory))
            {
                var breakdown = await _service.BreakdownAsync(directory, _includeHidden, ct);
                ct.ThrowIfCancellationRequested();
                ApplyBreakdown(breakdown);

                // The breakdown weighs different evidence and is the more specific answer for a
                // folder, so it wins where the two disagree.
                availability = breakdown.Availability;
            }
            else
            {
                Children.Clear();
                TotalBytes = null;
                UnknownChildCount = 0;
            }

            Availability = availability;
        }
        catch (OperationCanceledException)
        {
            // Superseded by a newer root; the newer call owns the view now.
            return;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            Children.Clear();
            LargestFiles.Clear();
            Availability = DiskUsageAvailability.NotAPath;
        }
        finally
        {
            if (ReferenceEquals(_cts, cts))
                IsLoading = false;
        }
    }

    private void ApplyBreakdown(DiskUsageBreakdown breakdown)
    {
        TotalBytes = breakdown.TotalBytes;
        UnknownChildCount = breakdown.UnknownChildCount;

        // Bars are scaled to the largest child rather than to the folder total, so a folder whose
        // own total is unknown still gets a readable comparison between its children.
        var largest = breakdown.Children.Count > 0 ? breakdown.Children.Max(c => c.SizeBytes ?? 0) : 0;

        Children.Clear();
        foreach (var child in breakdown.Children)
            Children.Add(new DiskUsageTileViewModel(child, largest));

        // The remainder gets a row only when it could be worked out honestly — see
        // DiskUsageRules.Unaccounted, which refuses to compute one from an incomplete sum.
        if (breakdown.UnaccountedBytes is { } unaccounted && unaccounted > 0)
        {
            Children.Add(new DiskUsageTileViewModel(
                new DiskUsageNode("", breakdown.RootDisplayPath, "Other files in this folder",
                    false, unaccounted, false, false),
                largest)
            { IsSynthetic = true });
        }
    }

    /// <summary>
    /// A volume finishing its index turns unknowns into numbers, so the open view re-queries — the
    /// same reason the folder tree refreshes its sizes rather than making the user reopen it.
    /// </summary>
    private void OnIndexRefreshed(string rootKey)
    {
        if (_disposed) return;

        var current = RootPath is { Length: > 0 } path ? PathKey.Canonicalize(path) : null;
        // A whole-PC view cares about every volume; a scoped one only about its own.
        if (current is not null &&
            !current.Equals(rootKey, StringComparison.Ordinal) &&
            !current.StartsWith(rootKey, StringComparison.Ordinal))
            return;

        Post(() => _ = LoadAsync(RootPath));
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

        _cts?.Cancel();
        _cts?.Dispose();
        _cts = null;
    }
}
