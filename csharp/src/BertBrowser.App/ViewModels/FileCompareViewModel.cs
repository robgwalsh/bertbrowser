using System.Collections.ObjectModel;
using System.IO;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Compare;
using BertBrowser.Core.Services.Diff;
using BertBrowser.Core.Services.Preview;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// One row of the side-by-side view.
/// </summary>
/// <remarks>
/// The same shape carries a text diff and a hex dump — a number and a run of text per side — so the
/// window has one list and one template rather than two of each. Plain rather than observable
/// because rows are rebuilt wholesale whenever anything changes.
/// </remarks>
public sealed class FileCompareRowViewModel(
    DiffOp op, string leftNumber, string leftText, string rightNumber, string rightText)
{
    public DiffOp Op { get; } = op;
    public string LeftNumber { get; } = leftNumber;
    public string LeftText { get; } = leftText;
    public string RightNumber { get; } = rightNumber;
    public string RightText { get; } = rightText;

    /// <summary>Whether each half is tinted. A deletion paints the left only, and vice versa.</summary>
    public bool LeftChanged => Op is DiffOp.Delete or DiffOp.Replace;

    public bool RightChanged => Op is DiffOp.Insert or DiffOp.Replace;
}

/// <summary>
/// Comparing two files: the verdict, and then either a line diff or a hex dump around the first
/// differing byte.
/// </summary>
public sealed partial class FileCompareViewModel : ObservableObject, IDisposable
{
    /// <summary>
    /// How much of each side is decoded for a text diff. Larger than the preview pane's budget,
    /// because a diff is the whole point here rather than a glance — but still bounded, and the
    /// window says when it bit.
    /// </summary>
    private const long TextBudget = 8L << 20;

    /// <summary>Rows of hex either side of the first difference. Enough to see what is around it.</summary>
    private const int HexContextRows = 24;

    private readonly IFileContentComparer _comparer;
    private CancellationTokenSource? _cts;
    private bool _disposed;

    private string[] _leftLines = [];
    private string[] _rightLines = [];
    private TextDiff? _diff;

    public FileCompareViewModel(IFileContentComparer comparer, string leftPath, string rightPath)
    {
        _comparer = comparer;
        LeftPath = leftPath;
        RightPath = rightPath;
    }

    [ObservableProperty]
    private string _leftPath = "";

    [ObservableProperty]
    private string _rightPath = "";

    public string LeftName => Path.GetFileName(LeftPath);

    public string RightName => Path.GetFileName(RightPath);

    public ObservableCollection<FileCompareRowViewModel> Rows { get; } = [];

    [ObservableProperty]
    private bool _isComparing = true;

    [ObservableProperty]
    private string _verdict = "";

    [ObservableProperty]
    private bool _isIdentical;

    [ObservableProperty]
    private string? _bannerMessage;

    [ObservableProperty]
    private FileCompareMode _mode = FileCompareMode.Text;

    [ObservableProperty]
    private string _differenceCounter = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanForceText))]
    private bool _forcedText;

    [ObservableProperty]
    private bool _ignoreWhitespace = true;

    [ObservableProperty]
    private bool _ignoreCase;

    public bool HasBanner => BannerMessage is not null;

    /// <summary>Offered only when the automatic answer was hex — there is nothing to force otherwise.</summary>
    public bool CanForceText => Mode is FileCompareMode.Binary && !ForcedText;

    public bool IsText => Mode is FileCompareMode.Text;

    /// <summary>Raised with a row index the view should bring into view.</summary>
    public event Action<int>? ScrollRequested;

    private int _hunkIndex = -1;

    // --- running the comparison ---

    public async Task CompareAsync()
    {
        _cts?.Cancel();
        _cts?.Dispose();
        var cts = new CancellationTokenSource();
        _cts = cts;

        IsComparing = true;
        BannerMessage = null;
        Rows.Clear();
        OnPropertyChanged(nameof(HasBanner));

        var left = LeftPath;
        var right = RightPath;

        ContentComparison? verdict = null;
        Loaded? loaded = null;

        try
        {
            verdict = await Task.Run(() => _comparer.Compare(left, right, null, cts.Token), CancellationToken.None);
            loaded = await Task.Run(() => Read(left, right), CancellationToken.None);
        }
        catch (OperationCanceledException)
        {
            return;
        }
        finally
        {
            if (ReferenceEquals(_cts, cts)) IsComparing = false;
        }

        if (_disposed || !ReferenceEquals(_cts, cts)) return;

        Apply(verdict, loaded, cts.Token);
    }

    private void Apply(ContentComparison? verdict, Loaded? loaded, CancellationToken ct)
    {
        if (verdict is null || loaded is null) return;

        IsIdentical = verdict.Identical;
        Verdict = Describe(verdict);

        if (verdict.Verdict is ContentVerdict.Unreadable)
        {
            BannerMessage = "One of these files could not be read, so they have not been compared.";
            OnPropertyChanged(nameof(HasBanner));
            return;
        }

        Mode = FileComparePlan.For(
            new FileCompareSide(LeftName, loaded.LeftConvincing),
            new FileCompareSide(RightName, loaded.RightConvincing),
            ForcedText);

        OnPropertyChanged(nameof(CanForceText));
        OnPropertyChanged(nameof(IsText));

        if (Mode is FileCompareMode.Text)
        {
            _leftLines = loaded.LeftLines;
            _rightLines = loaded.RightLines;
            BuildTextRows(loaded);
        }
        else
        {
            BuildHexRows(verdict, ct);
        }

        _hunkIndex = -1;
        UpdateCounter();
    }

    private void BuildTextRows(Loaded loaded)
    {
        var diff = TextDiffer.Compare(
            _leftLines, _rightLines,
            new DiffOptions(
                IgnoreTrailingWhitespace: IgnoreWhitespace,
                IgnoreAllWhitespace: false,
                IgnoreCase: IgnoreCase));

        _diff = diff;

        foreach (var row in diff.Rows)
        {
            Rows.Add(new FileCompareRowViewModel(
                row.Op,
                row.LeftLine > 0 ? row.LeftLine.ToString() : "",
                row.LeftLine > 0 ? _leftLines[row.LeftLine - 1] : "",
                row.RightLine > 0 ? row.RightLine.ToString() : "",
                row.RightLine > 0 ? _rightLines[row.RightLine - 1] : ""));
        }

        var notes = new List<string>();
        if (diff.BudgetExceeded)
            notes.Add("These files are too different to align line by line; they are shown side by side as they are.");
        if (loaded.Truncated)
            notes.Add($"Only the first {TextBudget / (1 << 20)} MB of each file was read.");

        BannerMessage = notes.Count > 0 ? string.Join(" ", notes) : null;
        OnPropertyChanged(nameof(HasBanner));
    }

    /// <summary>
    /// A window of hex either side of the first difference, on both files at once.
    /// </summary>
    /// <remarks>
    /// Dumping two whole binaries would be a hundred thousand rows nobody reads. The interesting
    /// place is the byte that differs, so the dump starts a couple of dozen rows before it — and the
    /// offsets shown are the real ones, which is what <c>HexPreviewReader</c>'s origin is for.
    /// </remarks>
    private void BuildHexRows(ContentComparison verdict, CancellationToken ct)
    {
        var start = verdict.FirstDifferenceOffset is { } offset
            ? Math.Max(0, (offset & ~0xFL) - (HexContextRows / 2L * HexPreviewReader.BytesPerRow))
            : 0;

        var left = Dump(LeftPath, start, ct);
        var right = Dump(RightPath, start, ct);

        for (var i = 0; i < Math.Max(left.Count, right.Count); i++)
        {
            var l = i < left.Count ? left[i] : default;
            var r = i < right.Count ? right[i] : default;

            var same = i < left.Count && i < right.Count && string.Equals(l.Text, r.Text, StringComparison.Ordinal);

            Rows.Add(new FileCompareRowViewModel(
                same ? DiffOp.Equal : DiffOp.Replace,
                "", i < left.Count ? l.Text : "",
                "", i < right.Count ? r.Text : ""));
        }

        if (Rows.Count == 0)
            BannerMessage = "Neither file could be read as bytes.";
        else if (start > 0)
            BannerMessage = $"Shown from byte {start:N0}, around the first difference.";

        OnPropertyChanged(nameof(HasBanner));
    }

    private static IReadOnlyList<HexRow> Dump(string path, long start, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();

        if (ReadOnlyFile.TryOpen(path) is not { } stream) return [];

        using (stream)
        {
            try
            {
                if (start > 0 && start < stream.Length) stream.Seek(start, SeekOrigin.Begin);
            }
            catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
            {
                return [];
            }

            var budget = (long)HexContextRows * 2 * HexPreviewReader.BytesPerRow;
            return HexPreviewReader.Read(stream, budget, HexContextRows * 2, origin: start).Rows;
        }
    }

    private sealed record Loaded(
        string[] LeftLines, string[] RightLines,
        bool LeftConvincing, bool RightConvincing, bool Truncated);

    /// <summary>
    /// Decodes both sides through <see cref="TextPreviewReader"/> — the app's only encoding ladder,
    /// so the preview pane and this window can never disagree about what a file is.
    /// </summary>
    private static Loaded Read(string leftPath, string rightPath)
    {
        var (leftLines, leftConvincing, leftTruncated) = ReadOne(leftPath);
        var (rightLines, rightConvincing, rightTruncated) = ReadOne(rightPath);

        return new Loaded(leftLines, rightLines, leftConvincing, rightConvincing, leftTruncated || rightTruncated);
    }

    private static (string[] Lines, bool Convincing, bool Truncated) ReadOne(string path)
    {
        if (ReadOnlyFile.TryOpen(path) is not { } stream) return ([], false, false);

        using (stream)
        {
            try
            {
                var preview = TextPreviewReader.Read(stream, TextBudget, maxLines: int.MaxValue);
                var lines = preview.Text.Length == 0 ? [] : preview.Text.Split('\n');

                return (lines, TextPreviewReader.IsConvincingText(preview), preview.Truncated);
            }
            catch (Exception ex) when (ReadOnlyFile.IsReadFailure(ex))
            {
                return ([], false, false);
            }
        }
    }

    private string Describe(ContentComparison verdict) => verdict.Verdict switch
    {
        ContentVerdict.Identical => $"Identical — {verdict.LeftBytes:N0} bytes",
        ContentVerdict.Unreadable => "Could not be compared",
        _ => verdict.FirstDifferenceOffset is { } offset
            ? $"Different — first difference at byte {offset:N0} "
              + $"(left {verdict.LeftBytes:N0}, right {verdict.RightBytes:N0} bytes)"
            : $"Different (left {verdict.LeftBytes:N0}, right {verdict.RightBytes:N0} bytes)",
    };

    // --- navigation ---

    private bool CanStep => _diff is { Hunks.Count: > 0 } || (Mode is FileCompareMode.Binary && Rows.Count > 0);

    [RelayCommand(CanExecute = nameof(CanStep))]
    private void NextDifference() => Step(+1);

    [RelayCommand(CanExecute = nameof(CanStep))]
    private void PreviousDifference() => Step(-1);

    private void Step(int direction)
    {
        if (_diff is not { Hunks.Count: > 0 } diff) return;

        _hunkIndex = _hunkIndex < 0
            ? direction > 0 ? 0 : diff.Hunks.Count - 1
            : Math.Clamp(_hunkIndex + direction, 0, diff.Hunks.Count - 1);

        UpdateCounter();
        ScrollRequested?.Invoke(diff.Hunks[_hunkIndex].FirstRow);
    }

    private void UpdateCounter()
    {
        var total = _diff?.Hunks.Count ?? 0;

        DifferenceCounter = total == 0
            ? IsIdentical ? "" : "—"
            : _hunkIndex < 0
                ? $"{total:N0} difference{(total == 1 ? "" : "s")}"
                : $"Difference {_hunkIndex + 1:N0} of {total:N0}";

        NextDifferenceCommand.NotifyCanExecuteChanged();
        PreviousDifferenceCommand.NotifyCanExecuteChanged();
    }

    // --- the toggles ---

    partial void OnIgnoreWhitespaceChanged(bool value) => Requery();

    partial void OnIgnoreCaseChanged(bool value) => Requery();

    /// <summary>
    /// Re-diffs from the lines already in memory. Cheap enough to do on a tick, which is why the
    /// toggles are checkboxes rather than something you press Apply on.
    /// </summary>
    private void Requery()
    {
        if (_disposed || Mode is not FileCompareMode.Text || _leftLines.Length + _rightLines.Length == 0) return;

        Rows.Clear();
        BuildTextRows(new Loaded(_leftLines, _rightLines, true, true, Truncated: false));
        _hunkIndex = -1;
        UpdateCounter();
    }

    [RelayCommand]
    private async Task ShowAsText()
    {
        ForcedText = true;
        await CompareAsync();
    }

    /// <summary>Swaps which file is on which side, and compares again.</summary>
    [RelayCommand]
    private async Task Swap()
    {
        (LeftPath, RightPath) = (RightPath, LeftPath);
        OnPropertyChanged(nameof(LeftName));
        OnPropertyChanged(nameof(RightName));
        await CompareAsync();
    }

    partial void OnBannerMessageChanged(string? value) => OnPropertyChanged(nameof(HasBanner));

    public void Dispose()
    {
        _disposed = true;
        _cts?.Cancel();
        _cts?.Dispose();
    }
}
