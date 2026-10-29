using System.IO;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Checksums;
using Microsoft.Win32;

namespace BertBrowser.App.Views;

/// <summary>
/// "What is this file's checksum, and does it match what the download page said?" — a selection
/// digested under any of five algorithms in one read, and the same window used to check a folder
/// against a <c>.sha256</c> or <c>.sfv</c>.
/// </summary>
/// <remarks>
/// Modeless, for the reason the duplicates window is: it outlives a run that can take minutes over
/// a large selection, and what a person does with a digest is compare it against something in
/// another application — a modal that owned the app while they went to find it would be the wrong
/// shape.
/// </remarks>
public partial class ChecksumWindow : ThemedWindow
{
    /// <remarks>
    /// Not disposed here. Whoever built the view model owns it, exactly as with
    /// <see cref="DuplicatesWindow"/> — a window that disposed what it was handed would kill a run
    /// the caller still wanted.
    /// </remarks>
    private readonly ChecksumViewModel _vm;

    public ChecksumWindow(ChecksumViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;

        // The picker is the view's job; the command's enablement is the view model's. This is the
        // seam between them, and it is why TrySave takes a path rather than opening anything.
        vm.SaveRequested += Save;
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static ChecksumWindow Create(ChecksumViewModel vm) => new(vm);

    /// <summary>Points the window at a selection and starts digesting it.</summary>
    /// <remarks>
    /// Unlike the duplicates window, this one runs on load rather than waiting to be asked. The user
    /// picked these exact files and asked for their checksums; there is no whole-PC surprise to
    /// guard against, and making them press a second button would be asking twice.
    /// </remarks>
    public void Load(IReadOnlyList<string> paths) => _vm.Load(paths);

    /// <summary>Points the window at a checksum file to check a folder against.</summary>
    public void LoadVerify(string checksumFilePath) => _vm.LoadVerify(checksumFilePath);

    private void Save()
    {
        var algorithm = _vm.SelectedAlgorithms.Count > 0
            ? _vm.SelectedAlgorithms[^1]
            : ChecksumAlgorithm.Sha256;

        var filter = string.Join("|", ChecksumAlgorithms.All.Select(a =>
            $"{ChecksumAlgorithms.DisplayName(a)} (*{ChecksumAlgorithms.FileExtension(a)})"
            + $"|*{ChecksumAlgorithms.FileExtension(a)}"));

        var dialog = new SaveFileDialog
        {
            Title = "Save checksum file",
            FileName = SuggestedName(algorithm),
            Filter = filter,
            FilterIndex = ChecksumAlgorithms.All.ToList().IndexOf(algorithm) + 1,
            DefaultExt = ChecksumAlgorithms.FileExtension(algorithm),
        };

        if (dialog.ShowDialog(this) != true) return;

        // The extension the user actually chose is what decides the format — picking ".md5" in the
        // dialog and getting a SHA-256 file would be the worst kind of surprise.
        var chosen = ChecksumAlgorithms.FromExtension(dialog.FileName) ?? algorithm;

        if (!_vm.TrySave(dialog.FileName, chosen, out var error))
        {
            MessageDialog.Show(this, error ?? "The checksum file could not be written.",
                "Save checksum file", MessageDialogKind.Warning);
            return;
        }

        if (error is not null)
            MessageDialog.Show(this, error, "Save checksum file", MessageDialogKind.Information);
    }

    private string SuggestedName(ChecksumAlgorithm algorithm)
    {
        var extension = ChecksumAlgorithms.FileExtension(algorithm);

        // One file gets its own name; a selection gets the folder's, which is what the checksum
        // file is actually about.
        if (_vm.Rows.Count == 1) return _vm.Rows[0].Name + extension;

        var folder = _vm.Rows.Count > 0 ? Path.GetDirectoryName(_vm.Rows[0].FullPath) : null;
        var stem = folder is { Length: > 0 } ? Path.GetFileName(folder) : "checksums";

        return (stem is { Length: > 0 } ? stem : "checksums") + extension;
    }
}
