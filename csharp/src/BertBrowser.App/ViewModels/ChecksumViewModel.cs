using System.Collections.ObjectModel;
using System.IO;
using System.Windows.Media;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.Checksums;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>What the checksum window is doing: producing digests, or checking them.</summary>
/// <remarks>
/// Two modes of one window rather than two windows, because the digester, the progress, the cancel
/// and the row list are identical and the only difference is one column. Two windows would be two
/// implementations of "show digests" that diverge the first time an algorithm is added.
/// </remarks>
public enum ChecksumMode { Hash, Verify }

/// <summary>One algorithm's answer for one file.</summary>
/// <remarks>
/// Shown — and copied — in the case its format is conventionally written in, which is the same rule
/// the checksum-file writer applies and the only rule about casing anywhere. Digests stay uppercase
/// inside Core, where the duplicate finder groups on them; a person reading one off a download page
/// is looking at lower case, and a digest that has to be mentally re-cased before it can be compared
/// by eye is a digest that will be compared wrongly.
/// </remarks>
public sealed partial class ChecksumDigestViewModel(ChecksumAlgorithm algorithm, string digest) : ObservableObject
{
    public ChecksumAlgorithm Algorithm { get; } = algorithm;
    public string Label { get; } = ChecksumAlgorithms.DisplayName(algorithm);

    public string Digest { get; } = ChecksumAlgorithms.IsWrittenLowercase(algorithm)
        ? digest.ToLowerInvariant()
        : digest.ToUpperInvariant();

    /// <summary>Set when the paste box holds this exact value — the "which of these is it?" answer.</summary>
    [ObservableProperty]
    private bool _isPasteMatch;

    [RelayCommand]
    private void Copy() => FileClipboard.TrySetText(Digest);
}

/// <summary>One file in the window.</summary>
public sealed partial class ChecksumRowViewModel(string path, string name) : ObservableObject
{
    public string FullPath { get; } = path;
    public string Name { get; } = name;
    public ImageSource? Icon => ShellIcons.GetIcon(FullPath, isDirectory: false);

    public ObservableCollection<ChecksumDigestViewModel> Digests { get; } = [];

    /// <summary>Every digest computed for this file so far, whether or not it is currently shown.</summary>
    public Dictionary<ChecksumAlgorithm, string> Computed { get; } = [];

    [ObservableProperty]
    private string? _errorMessage;

    [ObservableProperty]
    private ChecksumVerifyState? _verifyState;

    [ObservableProperty]
    private string? _verifyDetail;

    public bool HasError => ErrorMessage is not null;

    partial void OnErrorMessageChanged(string? value) => OnPropertyChanged(nameof(HasError));
}

/// <summary>
/// The checksum window's state: digest a selection under several algorithms, or verify a folder
/// against a checksum file.
/// </summary>
public sealed partial class ChecksumViewModel : ObservableObject, IDisposable
{
    private readonly IFileDigester _digester;
    private readonly ChecksumRunner _runner;

    private CancellationTokenSource? _cts;
    private bool _disposed;

    /// <summary>
    /// Set while the view model is ticking boxes itself, so its own writes are not mistaken for the
    /// user asking for a run. Verify mode sets the algorithm from the file being checked, and that
    /// must not read as "the user wants MD5 now".
    /// </summary>
    private bool _settingAlgorithms;

    /// <summary>Set in Verify mode: the parsed file, and the folder its names are relative to.</summary>
    private ChecksumFileParse? _verifying;
    private string _verifyFolder = "";
    private string _verifyFileName = "";

    public ChecksumViewModel(IFileDigester digester, IReadOnlyList<ChecksumAlgorithm> algorithms)
    {
        _digester = digester;
        _runner = new ChecksumRunner(digester);

        Algorithms = [.. ChecksumAlgorithms.All.Select(a =>
        {
            var choice = new ChecksumAlgorithmChoice(a) { IsSelected = algorithms.Contains(a) };
            choice.PropertyChanged += (_, e) =>
            {
                if (e.PropertyName == nameof(ChecksumAlgorithmChoice.IsSelected)) OnAlgorithmsChanged();
            };
            return choice;
        })];
    }

    public ObservableCollection<ChecksumRowViewModel> Rows { get; } = [];

    public IReadOnlyList<ChecksumAlgorithmChoice> Algorithms { get; }

    [ObservableProperty]
    private ChecksumMode _mode = ChecksumMode.Hash;

    [ObservableProperty]
    private string _title = "Checksums";

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RunCommand))]
    [NotifyCanExecuteChangedFor(nameof(CancelCommand))]
    [NotifyCanExecuteChangedFor(nameof(SaveCommand))]
    private bool _isRunning;

    [ObservableProperty]
    private bool _hasRun;

    [ObservableProperty]
    private string _progressHeadline = "";

    [ObservableProperty]
    private string _progressDetail = "";

    [ObservableProperty]
    private double _progressFraction;

    [ObservableProperty]
    private bool _isProgressIndeterminate = true;

    [ObservableProperty]
    private bool _wasCancelled;

    [ObservableProperty]
    private string? _bannerMessage;

    [ObservableProperty]
    private string _expectedInput = "";

    [ObservableProperty]
    private string? _pasteVerdict;

    [ObservableProperty]
    private bool _pasteMatched;

    [ObservableProperty]
    private string? _verifySummary;

    public bool HasBanner => BannerMessage is not null;

    public bool HasPasteVerdict => PasteVerdict is not null;

    /// <summary>The compare box only makes sense against digests that exist.</summary>
    public bool CanCompare => Rows.Count > 0 && Mode is ChecksumMode.Hash;

    /// <summary>What a blank list means, so an empty window always explains itself.</summary>
    public string EmptyMessage =>
        IsRunning ? ""
        : WasCancelled ? "Stopped before anything finished."
        : HasRun ? "Nothing to show."
        : "Select files and press Hash.";

    public bool IsEmpty => Rows.Count == 0;

    private IReadOnlyList<ChecksumAlgorithm> Selected =>
        [.. Algorithms.Where(a => a.IsSelected).Select(a => a.Algorithm)];

    public IReadOnlyList<ChecksumAlgorithm> SelectedAlgorithms => Selected;

    // --- loading ---

    /// <summary>Points the window at a selection of files to digest.</summary>
    public void Load(IReadOnlyList<string> paths)
    {
        Mode = ChecksumMode.Hash;
        _verifying = null;
        Title = paths.Count == 1 ? "Checksum" : $"Checksums — {paths.Count:N0} files";
        VerifySummary = null;
        BannerMessage = null;

        Rows.Clear();
        foreach (var path in paths)
        {
            Rows.Add(new ChecksumRowViewModel(path, Path.GetFileName(path) is { Length: > 0 } n ? n : path));
        }

        Notify();
        _ = RunAsync();
    }

    /// <summary>
    /// Points the window at a checksum file: parse it, then digest what it lists.
    /// </summary>
    public void LoadVerify(string checksumFilePath)
    {
        Mode = ChecksumMode.Verify;
        Title = $"Verify — {Path.GetFileName(checksumFilePath)}";
        VerifySummary = null;
        Rows.Clear();

        string text;
        try
        {
            text = File.ReadAllText(checksumFilePath);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            BannerMessage = "That checksum file could not be read.";
            Notify();
            return;
        }

        _verifyFileName = Path.GetFileName(checksumFilePath);
        _verifyFolder = Path.GetDirectoryName(checksumFilePath) ?? "";
        _verifying = ChecksumFile.Parse(text, _verifyFileName);

        if (_verifying.Lines.Count == 0)
        {
            BannerMessage = _verifying.Problems.Count > 0
                ? $"No checksum lines could be read from {_verifyFileName}."
                : $"{_verifyFileName} lists nothing.";
            Notify();
            return;
        }

        // The file's own algorithm is what it must be checked against; the tick boxes describe the
        // other mode. Selecting a different one here would be checking it against the wrong thing.
        var algorithm = _verifying.Algorithm ?? ChecksumAlgorithm.Sha256;

        _settingAlgorithms = true;
        foreach (var choice in Algorithms) choice.IsSelected = choice.Algorithm == algorithm;
        _settingAlgorithms = false;

        BannerMessage = _verifying.Problems.Count > 0
            ? $"{_verifying.Problems.Count:N0} line(s) in {_verifyFileName} could not be read and are not checked."
            : null;

        foreach (var line in _verifying.Lines)
        {
            var resolved = ChecksumPath.Resolve(_verifyFolder, line.Name);
            var row = new ChecksumRowViewModel(resolved ?? line.Name, line.Name);
            if (resolved is null)
            {
                row.VerifyState = ChecksumVerifyState.Refused;
                row.VerifyDetail = "This name points outside the folder and was not read.";
            }
            Rows.Add(row);
        }

        Notify();
        _ = RunAsync();
    }

    // --- running ---

    private bool CanRun => !IsRunning && Rows.Count > 0;

    [RelayCommand(CanExecute = nameof(CanRun))]
    private Task Run() => RunAsync(force: true);

    private bool CanCancel => IsRunning;

    [RelayCommand(CanExecute = nameof(CanCancel))]
    private void Cancel()
    {
        ProgressHeadline = "Stopping…";
        _cts?.Cancel();
    }

    private async Task RunAsync(bool force = false)
    {
        var wanted = Selected;
        if (wanted.Count == 0 || Rows.Count == 0)
        {
            Notify();
            return;
        }

        // Only what is actually missing. Ticking a fifth box after a run over 40 GB must not re-read
        // the other four algorithms' worth of bytes.
        var missing = force
            ? wanted
            : [.. wanted.Where(a => Rows.Any(r => r.VerifyState is not ChecksumVerifyState.Refused
                                                  && !r.Computed.ContainsKey(a)))];

        if (missing.Count == 0)
        {
            Project();
            return;
        }

        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;

        IsRunning = true;
        WasCancelled = false;
        ProgressHeadline = "Reading…";
        ProgressDetail = "";
        ProgressFraction = 0;
        IsProgressIndeterminate = true;
        Notify();

        var targets = Rows.Where(r => r.VerifyState is not ChecksumVerifyState.Refused).ToList();
        var paths = targets.Select(r => r.FullPath).ToList();
        var sizes = SizesOf(paths);
        var progress = new Progress<ChecksumProgress>(Apply);

        ChecksumOutcome? outcome = null;
        try
        {
            outcome = await Task.Run(
                () => _runner.Run(paths, missing, sizes, progress, cts.Token), CancellationToken.None);
        }
        catch (OperationCanceledException)
        {
            WasCancelled = true;
        }
        finally
        {
            if (ReferenceEquals(_cts, cts)) IsRunning = false;
        }

        if (_disposed || !ReferenceEquals(_cts, cts)) return;

        if (outcome is not null)
        {
            WasCancelled = outcome.Cancelled;

            for (var i = 0; i < outcome.Rows.Count && i < targets.Count; i++)
            {
                var result = outcome.Rows[i];
                var row = targets[i];

                if (result.Digests is null)
                {
                    row.ErrorMessage = "Could not be read — it may be locked, gone, or a cloud file "
                        + "that is not on this device.";
                    continue;
                }

                row.ErrorMessage = null;
                foreach (var (algorithm, digest) in result.Digests.ByAlgorithm)
                {
                    row.Computed[algorithm] = digest;
                }
            }
        }

        HasRun = true;
        Project();
        Reconcile();
        Notify();
    }

    /// <summary>Rebuilds each row's shown digests from what has been computed and what is ticked.</summary>
    private void Project()
    {
        var wanted = Selected;

        foreach (var row in Rows)
        {
            row.Digests.Clear();
            foreach (var algorithm in wanted)
            {
                if (row.Computed.TryGetValue(algorithm, out var digest))
                    row.Digests.Add(new ChecksumDigestViewModel(algorithm, digest));
            }
        }

        EvaluatePaste();
    }

    private void OnAlgorithmsChanged()
    {
        if (_disposed || _settingAlgorithms) return;

        Project();
        Notify();

        if (!IsRunning && Rows.Count > 0) _ = RunAsync();
    }

    private void Apply(ChecksumProgress progress)
    {
        if (_disposed || !IsRunning) return;

        ProgressHeadline = progress.FilesTotal > 1
            ? $"Reading {progress.FilesDone:N0} of {progress.FilesTotal:N0}…"
            : "Reading…";

        ProgressDetail = progress.CurrentName ?? "";

        if (progress.BytesTotal > 0)
        {
            IsProgressIndeterminate = false;
            ProgressFraction = Math.Clamp((double)progress.BytesDone / progress.BytesTotal, 0, 1);
        }
    }

    private static Dictionary<string, long> SizesOf(IEnumerable<string> paths)
    {
        var sizes = new Dictionary<string, long>(StringComparer.OrdinalIgnoreCase);
        foreach (var path in paths)
        {
            try
            {
                var info = new FileInfo(path);
                if (info.Exists) sizes[path] = info.Length;
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
            {
                // A size we cannot read costs a determinate bar, not the run.
            }
        }

        return sizes;
    }

    // --- verify ---

    private void Reconcile()
    {
        if (Mode is not ChecksumMode.Verify || _verifying is null) return;

        var algorithm = _verifying.Algorithm ?? ChecksumAlgorithm.Sha256;

        var computed = new Dictionary<string, string?>(StringComparer.OrdinalIgnoreCase);
        var refused = new List<string>();

        foreach (var row in Rows)
        {
            if (row.VerifyState is ChecksumVerifyState.Refused)
            {
                refused.Add(row.Name);
                continue;
            }

            if (row.Computed.TryGetValue(algorithm, out var digest)) computed[row.Name] = digest;
            else if (row.ErrorMessage is not null && File.Exists(row.FullPath)) computed[row.Name] = null;
        }

        var report = ChecksumVerify.Reconcile(_verifying.Lines, computed, refused, present: []);
        var byName = report.ToDictionary(r => r.Name, StringComparer.OrdinalIgnoreCase);

        foreach (var row in Rows)
        {
            if (row.VerifyState is ChecksumVerifyState.Refused) continue;
            if (!byName.TryGetValue(row.Name, out var result)) continue;

            row.VerifyState = result.State;
            row.VerifyDetail = result.State switch
            {
                ChecksumVerifyState.Mismatch => $"expected {result.Expected?.ToLowerInvariant()}",
                ChecksumVerifyState.Missing => "not found in this folder",
                ChecksumVerifyState.Unreadable => "could not be read",
                _ => null,
            };

            // The state and its detail already say why there is no digest, and say it better. The
            // digester's own "could not be read" underneath would be a second, vaguer answer to the
            // same question — and against a Missing row, a contradictory one.
            if (result.State is not ChecksumVerifyState.Ok) row.ErrorMessage = null;
        }

        VerifySummary = ChecksumVerify.Summarise(report);
    }

    // --- the paste box ---

    partial void OnExpectedInputChanged(string value) => EvaluatePaste();

    /// <summary>
    /// Works out what was pasted and which file it belongs to.
    /// </summary>
    /// <remarks>
    /// The length of a digest names its algorithm — no two of the five share one — so pasting a
    /// checksum from a download page selects the algorithm by itself, and computes it if it was not
    /// already ticked. With one file this answers "is this the right file?"; with many it answers
    /// the better question, "which of these is this hash for?".
    /// </remarks>
    private void EvaluatePaste()
    {
        foreach (var row in Rows)
        {
            foreach (var digest in row.Digests) digest.IsPasteMatch = false;
        }

        var pasted = ExpectedInput.Trim();
        if (pasted.Length == 0 || Rows.Count == 0)
        {
            PasteVerdict = null;
            PasteMatched = false;
            OnPropertyChanged(nameof(HasPasteVerdict));
            return;
        }

        var digestText = ChecksumCompare.ExtractDigest(pasted);
        if (digestText is null)
        {
            PasteVerdict = "That does not look like a checksum.";
            PasteMatched = false;
            OnPropertyChanged(nameof(HasPasteVerdict));
            return;
        }

        var algorithm = ChecksumAlgorithms.Recognise(digestText);
        if (algorithm is { } needed && Algorithms.FirstOrDefault(a => a.Algorithm == needed) is { IsSelected: false } choice)
        {
            // Tick it and let the change handler compute it — the paste has said which algorithm the
            // user is actually working in, so asking them to tick it themselves is asking twice.
            choice.IsSelected = true;
            return;
        }

        ChecksumRowViewModel? match = null;
        foreach (var row in Rows)
        {
            foreach (var digest in row.Digests)
            {
                if (!string.Equals(digest.Digest, digestText, StringComparison.OrdinalIgnoreCase)) continue;

                digest.IsPasteMatch = true;
                match ??= row;
            }
        }

        PasteMatched = match is not null;
        PasteVerdict = match is not null
            ? Rows.Count == 1 ? "✓ Matches" : $"✓ Matches {match.Name}"
            : Rows.Count == 1 ? "✗ Does not match" : "✗ Matches nothing here";

        OnPropertyChanged(nameof(HasPasteVerdict));
    }

    // --- writing a checksum file ---

    private bool CanSave => !IsRunning && Rows.Any(r => r.Computed.Count > 0);

    [RelayCommand(CanExecute = nameof(CanSave))]
    private void Save()
    {
        // The view drives the picker; this exists so the command's enablement lives with the state.
        SaveRequested?.Invoke();
    }

    public event Action? SaveRequested;

    /// <summary>
    /// Writes a checksum file for everything digested, relative to the file's own folder.
    /// </summary>
    /// <returns>False with a reason, rather than throwing — the caller shows it.</returns>
    public bool TrySave(string path, ChecksumAlgorithm algorithm, out string? error)
    {
        var folder = Path.GetDirectoryName(path);
        if (folder is not { Length: > 0 })
        {
            error = "That is not a folder a checksum file can be written to.";
            return false;
        }

        var lines = new List<ChecksumLine>();
        var outside = 0;

        foreach (var row in Rows)
        {
            if (!row.Computed.TryGetValue(algorithm, out var digest)) continue;

            var name = ChecksumPath.Relativise(folder, row.FullPath);
            if (name is null)
            {
                outside++;
                continue;
            }

            lines.Add(new ChecksumLine(name, digest));
        }

        if (lines.Count == 0)
        {
            error = outside > 0
                ? "None of these files are under the folder the checksum file is being saved to, so "
                  + "nothing could be listed relative to it."
                : $"No {ChecksumAlgorithms.DisplayName(algorithm)} digests have been computed yet.";
            return false;
        }

        try
        {
            File.WriteAllText(path, ChecksumFile.Render(algorithm, lines));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            error = $"The checksum file could not be written: {ex.Message}";
            return false;
        }

        error = outside > 0
            ? $"{outside:N0} file(s) are not under this folder and were left out."
            : null;
        return true;
    }

    /// <summary>Copies every shown digest, one file per block.</summary>
    [RelayCommand]
    private void CopyAll()
    {
        var text = new System.Text.StringBuilder();

        foreach (var row in Rows)
        {
            foreach (var digest in row.Digests)
            {
                text.Append(digest.Digest.ToLowerInvariant()).Append("  ").Append(row.Name).Append('\n');
            }
        }

        if (text.Length > 0) FileClipboard.TrySetText(text.ToString());
    }

    private void Notify()
    {
        OnPropertyChanged(nameof(IsEmpty));
        OnPropertyChanged(nameof(EmptyMessage));
        OnPropertyChanged(nameof(HasBanner));
        OnPropertyChanged(nameof(CanCompare));
        RunCommand.NotifyCanExecuteChanged();
        SaveCommand.NotifyCanExecuteChanged();
    }

    partial void OnBannerMessageChanged(string? value) => OnPropertyChanged(nameof(HasBanner));

    partial void OnIsRunningChanged(bool value) => Notify();

    partial void OnModeChanged(ChecksumMode value) => OnPropertyChanged(nameof(CanCompare));

    public void Dispose()
    {
        _disposed = true;
        _cts?.Cancel();
        _cts?.Dispose();
    }
}

/// <summary>One tick box in the algorithm row.</summary>
public sealed partial class ChecksumAlgorithmChoice(ChecksumAlgorithm algorithm) : ObservableObject
{
    public ChecksumAlgorithm Algorithm { get; } = algorithm;

    public string Label { get; } = ChecksumAlgorithms.DisplayName(algorithm);

    /// <summary>
    /// Said out loud beside CRC-32 only. It is in the list because .sfv files exist and people have
    /// them, not because it is a way to tell whether a download was tampered with — and a tool that
    /// offers it silently alongside SHA-256 implies otherwise.
    /// </summary>
    public string? Note { get; } = algorithm is ChecksumAlgorithm.Crc32
        ? "for .sfv files; catches corruption, not tampering"
        : null;

    public bool HasNote => Note is not null;

    [ObservableProperty]
    private bool _isSelected;

}
