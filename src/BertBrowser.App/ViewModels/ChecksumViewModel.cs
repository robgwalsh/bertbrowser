using System.Windows.Media;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.Duplicates;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>Backing VM for <see cref="Views.ChecksumDialog"/>: hashes one file with the same
/// <see cref="IFileHasher"/> the duplicate finder uses, and compares it against a value the user
/// pastes in (e.g. from a download page).</summary>
public sealed partial class ChecksumViewModel : ObservableObject, IDisposable
{
    private readonly IFileHasher _hasher;
    private readonly CancellationTokenSource _cts = new();

    public string FullPath { get; }
    public string Name { get; }
    public ImageSource? Icon => ShellIcons.GetIcon(FullPath, isDirectory: false);

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CopyDigestCommand))]
    private string? _digest;

    [ObservableProperty]
    private bool _isHashing = true;

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    private string _expectedInput = "";

    [ObservableProperty]
    private ChecksumMatchState _matchState;

    public ChecksumViewModel(string fullPath, IFileHasher hasher)
    {
        FullPath = fullPath;
        Name = System.IO.Path.GetFileName(fullPath) is { Length: > 0 } name ? name : fullPath;
        _hasher = hasher;
    }

    public async Task HashAsync()
    {
        try
        {
            var fingerprint = await Task.Run(
                () => _hasher.Hash(FullPath, maxBytes: 0, progress: null, _cts.Token), _cts.Token);
            Digest = fingerprint?.Hash;
            if (fingerprint is null)
                ErrorMessage = "Could not read this file — it may be unreadable, a cloud file not "
                    + "downloaded to this device, or in use in a way that blocks reading.";
        }
        catch (OperationCanceledException)
        {
            // The dialog closed before the hash finished; nothing left to show.
        }
        finally
        {
            IsHashing = false;
        }
    }

    partial void OnExpectedInputChanged(string value) => MatchState = ChecksumCompare.Evaluate(Digest, value);

    partial void OnDigestChanged(string? value) => MatchState = ChecksumCompare.Evaluate(value, ExpectedInput);

    private bool CanCopyDigest => Digest is not null;

    [RelayCommand(CanExecute = nameof(CanCopyDigest))]
    private void CopyDigest()
    {
        if (Digest is { } digest) FileClipboard.TrySetText(digest);
    }

    public void Dispose() => _cts.Cancel();
}
