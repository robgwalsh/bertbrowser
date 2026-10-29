using System.Collections.ObjectModel;
using System.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Duplicates;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// One copy of a duplicated file, and whether the user has marked it to go.
/// </summary>
/// <remarks>
/// The row itself is a <see cref="FileItemViewModel"/> built by the same factory the search results
/// and the disk-usage list use, so the icon, the size and the timestamp formatting cannot drift
/// between the three.
/// </remarks>
public sealed partial class DuplicateFileViewModel : ObservableObject
{
    public DuplicateFileViewModel(DuplicateFile file)
    {
        Source = file;
        Item = FileListViewModel.CreateSearchItem(new SearchHit(
            file.DisplayPath, file.RelativeDirDisplay, file.Name,
            false, file.SizeBytes, file.ModifiedUtc, file.Hidden));
    }

    public DuplicateFile Source { get; }

    public FileItemViewModel Item { get; }

    public string FullPath => Source.DisplayPath;

    /// <summary>The folder this copy sits in — which is the only thing that tells the copies apart.</summary>
    public string FolderDisplay => Path.GetDirectoryName(Source.DisplayPath) ?? Source.DisplayPath;

    public bool IsHardlinked => Source.HardlinkPaths.Count > 0;

    /// <summary>
    /// What the other names for this same file are, when it has any. Said out loud because it
    /// changes what deleting means: these are not copies, and removing one reclaims nothing.
    /// </summary>
    public string HardlinkNote => Source.HardlinkPaths.Count switch
    {
        0 => "",
        1 => $"Also linked as {Source.HardlinkPaths[0]}",
        var n => $"Also linked under {n} other names",
    };

    [ObservableProperty]
    private bool _isTicked;

    /// <summary>
    /// False for the last copy a group has left. Disabling it is how the "one copy always stays"
    /// rule is shown rather than enforced by a checkbox that springs back, which reads as a bug.
    /// </summary>
    [ObservableProperty]
    private bool _canTick = true;
}

/// <summary>
/// A set of files that are byte-for-byte the same thing in different places.
/// </summary>
public sealed partial class DuplicateGroupViewModel : ObservableObject
{
    private bool _updating;

    public DuplicateGroupViewModel(DuplicateGroup group)
    {
        ArgumentNullException.ThrowIfNull(group);
        Source = group;

        foreach (var file in group.Files)
        {
            var row = new DuplicateFileViewModel(file);
            row.PropertyChanged += OnFilePropertyChanged;
            Files.Add(row);
        }

        UpdateCounts();
    }

    public DuplicateGroup Source { get; }

    public ObservableCollection<DuplicateFileViewModel> Files { get; } = [];

    public long SizeBytes => Source.SizeBytes;

    public string SizeDisplay => ByteSizeFormatter.Format(Source.SizeBytes);

    public string Header => $"{Files.Count} copies · {SizeDisplay} each";

    /// <summary>What removing all but one would reclaim — never the whole group's bytes.</summary>
    public long WastedBytes => SizeBytes * Math.Max(0, Files.Count - 1);

    public string WastedDisplay => $"{ByteSizeFormatter.Format(WastedBytes)} reclaimable";

    [ObservableProperty]
    private int _tickedCount;

    /// <summary>Raised when a tick changes, so the window can re-total without watching every row.</summary>
    public event Action? TicksChanged;

    /// <summary>
    /// Marks every copy but the one <paramref name="strategy"/> chooses.
    /// </summary>
    /// <remarks>
    /// The keeper comes from <see cref="DuplicateRules.ChooseKeeper"/> rather than being decided
    /// here, so the same group always yields the same answer — an auto-selection that shuffled
    /// between presses would be impossible to trust with a delete on the end of it.
    /// </remarks>
    public void TickAllBut(KeepStrategy strategy)
    {
        var keeper = DuplicateRules.ChooseKeeper(Source, strategy);

        _updating = true;
        for (var i = 0; i < Files.Count; i++) Files[i].IsTicked = i != keeper;
        _updating = false;

        UpdateCounts();
    }

    public void ClearTicks()
    {
        _updating = true;
        foreach (var file in Files) file.IsTicked = false;
        _updating = false;

        UpdateCounts();
    }

    /// <summary>
    /// Drops copies that have gone, after a delete. The group is not re-scanned: the files that
    /// remain were confirmed identical by this run and still are.
    /// </summary>
    /// <returns>False once fewer than two copies are left — the group has stopped being one.</returns>
    public bool Remove(IReadOnlyCollection<string> paths)
    {
        ArgumentNullException.ThrowIfNull(paths);

        var gone = Files
            .Where(f => paths.Contains(f.FullPath, StringComparer.OrdinalIgnoreCase))
            .ToList();

        foreach (var file in gone)
        {
            file.PropertyChanged -= OnFilePropertyChanged;
            Files.Remove(file);
        }

        UpdateCounts();
        OnPropertyChanged(nameof(Header));
        OnPropertyChanged(nameof(WastedBytes));
        OnPropertyChanged(nameof(WastedDisplay));

        return Files.Count > 1;
    }

    private void OnFilePropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (_updating || e.PropertyName != nameof(DuplicateFileViewModel.IsTicked)) return;
        UpdateCounts();
    }

    /// <summary>
    /// Recounts and re-decides which boxes may still be ticked. The last unticked copy locks: the
    /// point of the feature is to reclaim what a redundant copy costs, and a group with every box
    /// filled would destroy the only remaining instance of a file the user was just told they had
    /// several of.
    /// </summary>
    private void UpdateCounts()
    {
        var ticked = Files.Count(f => f.IsTicked);
        TickedCount = ticked;

        var lastOneStanding = Files.Count - ticked <= 1;
        foreach (var file in Files) file.CanTick = file.IsTicked || !lastOneStanding;

        TicksChanged?.Invoke();
    }
}
