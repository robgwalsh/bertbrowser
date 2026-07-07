using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>One in-flight directory size scan, surfaced in the scan-progress dialog. Its
/// counters are updated live from the <see cref="DirScanProgress"/> reports; cancelling
/// trips the scan's own linked token (see <see cref="ShellViewModel"/>) so a single row can
/// stop without affecting the others.</summary>
public sealed partial class ScanProgressViewModel : ObservableObject
{
    private readonly CancellationTokenSource _cts;

    public string FolderPath { get; }
    public string FolderName { get; }

    [ObservableProperty]
    private int _directoriesScanned;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BytesDisplay))]
    private long _bytesSoFar;

    [ObservableProperty]
    private string _currentDirectory = "";

    [ObservableProperty]
    private bool _isCancelling;

    public string BytesDisplay => ByteSizeFormatter.Format(BytesSoFar);

    public ScanProgressViewModel(string folderPath, CancellationTokenSource cts)
    {
        _cts = cts;
        FolderPath = folderPath;
        var name = Path.GetFileName(folderPath.TrimEnd('\\'));
        FolderName = name.Length > 0 ? name : folderPath; // drive roots have no file name
    }

    [RelayCommand]
    private void Cancel()
    {
        IsCancelling = true;
        _cts.Cancel();
    }
}
