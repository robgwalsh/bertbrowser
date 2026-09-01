using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Compare;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// One comparison, running across two open panes.
/// </summary>
/// <remarks>
/// <para>
/// It owns the answer; the rows only display it. A row is rebuilt whenever its file changes on
/// disk, so it cannot be what remembers a verdict — instead each list is handed a stamp
/// (<c>FileListViewModel.RowState</c>) that is asked as every row is built. The session outlives
/// any number of loads, refreshes and navigations within the two folders it compared.
/// </para>
/// <para>
/// Kept out of <c>ShellViewModel</c>, which is long enough already, and because this has a lifetime
/// of its own with rules about when it ends — the interesting part of the feature.
/// </para>
/// </remarks>
public sealed partial class CompareSessionViewModel : ObservableObject, IDisposable
{
    private readonly IFolderCompareService _service;
    private readonly Func<bool> _includeHidden;
    private CancellationTokenSource? _cts;
    private bool _ended;

    public DirectoryTabViewModel Left { get; }

    public DirectoryTabViewModel Right { get; }

    /// <summary>
    /// The folders the comparison was actually run against — not the tabs' current paths, which
    /// move underneath it as the user walks into subfolders.
    /// </summary>
    public string LeftRoot { get; }

    public string RightRoot { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Summary), nameof(HasResult))]
    [NotifyCanExecuteChangedFor(nameof(SyncCommand))]
    private FolderCompareOutcome? _result;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RescanCommand), nameof(SyncCommand))]
    private bool _isComparing;

    /// <summary>Something under one of the two roots changed since the scan, so the verdicts on
    /// screen are older than the disk. The banner offers a rescan rather than quietly re-running
    /// one: a comparison of a large tree is not free, and nothing here is urgent.</summary>
    [ObservableProperty]
    private bool _isStale;

    /// <summary>Shows only the rows the comparison found something to say about, on both sides at
    /// once — comparing is a two-sided idea and filtering one list would be half an answer.</summary>
    [ObservableProperty]
    private bool _differencesOnly;

    /// <summary>What went wrong, or what to be careful of. Null when there is nothing to say.</summary>
    [ObservableProperty]
    private string? _message;

    public bool HasResult => Result is { Availability: not CompareAvailability.Refused };

    public event Action<CompareSessionViewModel>? Ended;

    /// <summary>Asks for the sync preview. An event because a view model does not build windows,
    /// the same shape <c>ShellViewModel.DiskUsageRequested</c> has.</summary>
    public event Action<CompareSessionViewModel>? SyncRequested;

    public CompareSessionViewModel(
        IFolderCompareService service,
        DirectoryTabViewModel left,
        DirectoryTabViewModel right,
        Func<bool> includeHidden)
    {
        _service = service;
        _includeHidden = includeHidden;
        Left = left;
        Right = right;
        LeftRoot = left.CurrentPath;
        RightRoot = right.CurrentPath;

        Left.LocationChanged += OnLocationChanged;
        Right.LocationChanged += OnLocationChanged;
        Left.Closing += OnTabClosing;
        Right.Closing += OnTabClosing;
        Left.FileList.PropertyChanged += OnFileListChanged;
        Right.FileList.PropertyChanged += OnFileListChanged;
    }

    public string Summary
    {
        get
        {
            if (Result is not { } outcome) return "";
            if (outcome.Problem is { Length: > 0 } problem) return problem;

            var result = outcome.Result;
            if (!result.AnyDifference) return "These folders match.";

            // Counted the way the sync plans, so the banner and the preview cannot disagree: a
            // folder the other side lacks is one thing, however much is inside it, and a folder
            // both sides have is not a difference of its own at all.
            var parts = new List<string>();
            var left = result.Count(CompareVerdict.LeftOnly);
            var right = result.Count(CompareVerdict.RightOnly);
            var newer = result.Count(CompareVerdict.LeftNewer);
            var older = result.Count(CompareVerdict.RightNewer);
            var differs = result.Count(CompareVerdict.Differs);

            if (left > 0) parts.Add($"{left} only on the left");
            if (right > 0) parts.Add($"{right} only on the right");
            if (newer > 0) parts.Add($"{newer} newer on the left");
            if (older > 0) parts.Add($"{older} newer on the right");
            if (differs > 0) parts.Add($"{differs} differ");
            // Said last and always, when there are any: an entry nothing is known about is the one
            // the sync will not touch, and it must not be discovered afterwards.
            var unknown = result.Count(CompareVerdict.Unknown);
            if (unknown > 0) parts.Add($"{unknown} could not be compared");

            return string.Join(" · ", parts);
        }
    }

    // --- Running it ---

    [RelayCommand(CanExecute = nameof(CanRescan))]
    public async Task RescanAsync()
    {
        if (_ended) return;

        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;

        IsComparing = true;
        Message = null;
        try
        {
            var outcome = await _service.CompareAsync(
                LeftRoot, RightRoot, _includeHidden(), cts.Token);

            // A run that was superseded does not own the view any more.
            if (!ReferenceEquals(_cts, cts) || _ended) return;

            Result = outcome;
            IsStale = false;
            Message = Caveat(outcome);
            Attach();
        }
        finally
        {
            if (ReferenceEquals(_cts, cts)) IsComparing = false;
        }
    }

    private bool CanRescan() => !IsComparing;

    /// <summary>What to warn about beyond the summary. Each of these leaves the verdicts usable but
    /// changes what may be done with them.</summary>
    private static string? Caveat(FolderCompareOutcome outcome) => outcome switch
    {
        { Availability: CompareAvailability.Refused or CompareAvailability.Unreadable } =>
            outcome.Problem,
        { Truncated: true } =>
            "These folders hold more entries than one comparison can take, so this is only part of " +
            "them — syncing is off.",
        { Availability: CompareAvailability.Building } =>
            "A drive is still being indexed, so something may yet be missing.",
        _ => null,
    };

    [RelayCommand(CanExecute = nameof(CanSync))]
    private void Sync() => SyncRequested?.Invoke(this);

    private bool CanSync() => !IsComparing && Result?.CanSync == true;

    // --- Stamping the rows ---

    private void Attach()
    {
        // Assigning the stamp re-stamps and rebuilds the header, so this is also what puts the
        // Status column up.
        Left.FileList.RowState = row => Stamp(row, CompareSide.Left);
        Right.FileList.RowState = row => Stamp(row, CompareSide.Right);
        ApplyFilter();
    }

    private CompareRowState Stamp(FileItemViewModel row, CompareSide side)
    {
        if (Result?.Result is not { } result) return CompareRowState.None;

        var root = side is CompareSide.Left ? LeftRoot : RightRoot;
        if (RelativeKeyOf(row.FullPath, root) is not { } key) return CompareRowState.None;

        var entries = side is CompareSide.Left ? result.Left : result.Right;
        if (!entries.TryGetValue(key, out var entry)) return CompareRowState.Unknown;

        // The row is describing a file the comparison did not see. Keeping the old verdict would be
        // stating something that stopped being true, so it becomes an unknown and the banner offers
        // a rescan — the one thing it must not do is stay green.
        if (HasMoved(entry, row))
        {
            IsStale = true;
            return CompareRowState.Unknown;
        }

        return CompareRules.RowState(result.For(key), side);
    }

    /// <summary>Whether a row's file is no longer the one that was compared. Only asked of files:
    /// a folder's size comes from a cache rather than from the listing, and its own timestamp moves
    /// for reasons a comparison never weighed.</summary>
    private static bool HasMoved(CompareEntry entry, FileItemViewModel row) =>
        !row.IsDirectory
        && row.SizeBytes is { } size
        && (size != entry.SizeBytes || row.ModifiedUtc != entry.ModifiedUtc);

    /// <summary>The row's path relative to its side's root, or null when it is not under it at all
    /// — which is what a row in a search result or another folder is.</summary>
    private static string? RelativeKeyOf(string fullPath, string root)
    {
        if (fullPath.Length == 0 || root.Length == 0) return null;

        try
        {
            var key = PathKey.Canonicalize(fullPath);
            var (lo, _) = PathKey.PrefixBounds(PathKey.Canonicalize(root));
            return PathKey.IsUnder(key, root) ? key[lo.Length..] : null;
        }
        catch (ArgumentException)
        {
            return null; // a virtual or malformed path is simply not part of this comparison
        }
    }

    partial void OnDifferencesOnlyChanged(bool value) => ApplyFilter();

    private void ApplyFilter()
    {
        Left.FileList.DifferencesOnly = DifferencesOnly;
        Right.FileList.DifferencesOnly = DifferencesOnly;
    }

    // --- When it ends ---

    /// <summary>
    /// A tab that stayed inside the folder it compared keeps the session; one that left ends it.
    /// </summary>
    /// <remarks>
    /// Staying is what makes walking into <c>left\src</c> and <c>right\src</c> work with no rescan:
    /// the result is keyed by relative path, so a subfolder is already answered. Leaving has to end
    /// it, because silently re-rooting the comparison onto wherever the tab went would leave a Sync
    /// button pointing at a pairing nobody asked for.
    /// </remarks>
    private void OnLocationChanged(DirectoryTabViewModel tab)
    {
        var root = ReferenceEquals(tab, Left) ? LeftRoot : RightRoot;
        if (IsInside(tab.CurrentPath, root)) return;

        End($"Compare ended — a pane left {root}.");
    }

    private static bool IsInside(string path, string root)
    {
        if (path.Length == 0 || root.Length == 0) return false;
        try
        {
            var key = PathKey.Canonicalize(path);
            var rootKey = PathKey.Canonicalize(root);
            return string.Equals(key, rootKey, StringComparison.Ordinal) || PathKey.IsUnder(key, rootKey);
        }
        catch (ArgumentException)
        {
            return false;
        }
    }

    private void OnTabClosing(DirectoryTabViewModel tab) => End("Compare ended — a pane was closed.");

    /// <summary>A search flattens the list into hits from all over the tree, which is not a folder
    /// listing and cannot be compared against one.</summary>
    private void OnFileListChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(FileListViewModel.IsFlattened)) return;
        if (sender is FileListViewModel { IsFlattened: true })
            End("Compare ended — a pane started a search.");
    }

    [RelayCommand]
    private void EndByRequest() => End(null);

    /// <summary>Ends the session and gives the two lists back. Idempotent: several of the things
    /// that end a comparison happen together when a pane closes.</summary>
    public void End(string? reason)
    {
        if (_ended) return;
        _ended = true;

        _cts?.Cancel();

        Left.LocationChanged -= OnLocationChanged;
        Right.LocationChanged -= OnLocationChanged;
        Left.Closing -= OnTabClosing;
        Right.Closing -= OnTabClosing;
        Left.FileList.PropertyChanged -= OnFileListChanged;
        Right.FileList.PropertyChanged -= OnFileListChanged;

        foreach (var list in new[] { Left.FileList, Right.FileList })
        {
            list.DifferencesOnly = false;
            list.RowState = null;   // re-stamps every row back to None and drops the Status column
        }

        if (reason is { Length: > 0 })
            Left.StatusText = reason;

        Ended?.Invoke(this);
    }

    public void Dispose()
    {
        End(null);
        _cts?.Dispose();
    }
}
