using System.Windows;
using System.Windows.Media;
using BertBrowser.App.Interop;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Theming;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.Views;

/// <summary>
/// The "are you sure" for a delete. It names every item, says where each one lives, and — as the
/// survey walks the trees — how much each one actually amounts to, because "delete 1 item" and
/// "delete 1 folder holding 40,000 files" are not the same question.
/// </summary>
/// <remarks>
/// The dialog shows the same <see cref="DeletePlan"/> the executor is handed, so what is listed is
/// what goes: items the planner refused are called out here rather than turning into a silently
/// shorter delete. Measuring runs off the UI thread and is cancelled when the dialog closes, so a
/// huge folder never holds the answer up — the list and the buttons are usable immediately, and the
/// totals fill in behind them.
/// </remarks>
public partial class DeleteDialog : ThemedWindow
{
    /// <summary>How many refusals to spell out before summarising the rest.</summary>
    private const int ProblemLines = 3;

    /// <summary>Measures a plan. Matches <c>ShellViewModel.SurveyDelete</c>, so the dialog never
    /// has to hold a view model to ask.</summary>
    public delegate DeleteSurvey Surveyor(
        DeletePlan plan, CancellationToken ct, IProgress<DeleteMeasurement>? progress);

    private readonly CancellationTokenSource _surveying = new();
    private readonly Dictionary<string, DeleteItemRow> _rows = new(StringComparer.Ordinal);

    private long _bytes;
    private int _files;
    private int _directories;
    private int _measured;
    private bool _incomplete;

    private DeleteDialog(DeletePlan plan, Surveyor surveyor)
    {
        InitializeComponent();

        Title = plan.Permanent ? "Delete permanently" : "Delete";
        // Segoe Fluent Icons: Warning for the ordinary case, the heavier Error mark when there is
        // no way back.
        Glyph.Text = char.ConvertFromUtf32(plan.Permanent ? 0xE783 : 0xE7BA);
        if (plan.Permanent && TryFindResource(ThemeToken.ErrorForeground) is Brush error)
            Glyph.Foreground = error;

        HeadingText.Text = Heading(plan);
        PermanentBanner.Visibility = plan.Permanent ? Visibility.Visible : Visibility.Collapsed;
        UndoHint.Text = plan.Permanent
            ? ""
            : "Ctrl+Z puts these back. They are held until the next move, rename or delete — or " +
              "until BertBrowser closes — and then removed for good.";

        foreach (var item in plan.Deletions)
        {
            var row = new DeleteItemRow(item);
            _rows[PathKey.Canonicalize(item.SourcePath)] = row;
            ItemList.Items.Add(row);
        }

        ShowProblems(plan);
        TotalsText.Text = "Adding up what this would remove…";
        StartSurvey(plan, surveyor);

        Loaded += (_, _) => CancelButton.Focus();
        Closed += (_, _) => _surveying.Cancel();
    }

    /// <summary>Shows the dialog. True when the user chose to go ahead.</summary>
    public static bool Confirm(Window? owner, DeletePlan plan, Surveyor surveyor)
    {
        if (!plan.HasWork) return false;

        var dialog = new DeleteDialog(plan, surveyor);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true;
    }

    private static string Heading(DeletePlan plan)
    {
        var verb = plan.Permanent ? "Permanently delete" : "Delete";
        return plan.Deletions.Count == 1
            ? $"{verb} '{plan.Deletions[0].Name}'?"
            : $"{verb} these {plan.Deletions.Count:N0} items?";
    }

    private void ShowProblems(DeletePlan plan)
    {
        var problems = plan.Problems;
        if (problems.Count == 0) return;

        var text = string.Join("\n", problems.Take(ProblemLines).Select(p => p.Message));
        if (problems.Count > ProblemLines)
            text += $"\n…and {problems.Count - ProblemLines:N0} more.";

        ProblemText.Text = $"{problems.Count:N0} selected item(s) will not be deleted:\n{text}";
        ProblemBanner.Visibility = Visibility.Visible;
    }

    // --- Survey ---

    private void StartSurvey(DeletePlan plan, Surveyor surveyor)
    {
        // Marshalled by Progress<T>, which captures this thread's dispatcher context here in the
        // constructor — so the handler runs on the UI thread and can touch the rows directly.
        var progress = new Progress<DeleteMeasurement>(Apply);
        var token = _surveying.Token;
        _ = Task.Run(() => surveyor(plan, token, progress), token);
    }

    private void Apply(DeleteMeasurement measurement)
    {
        if (_surveying.IsCancellationRequested) return;

        _bytes += measurement.Bytes;
        _files += measurement.Files;
        _directories += measurement.Directories;
        _incomplete |= measurement.Incomplete;
        _measured++;

        if (_rows.TryGetValue(PathKey.Canonicalize(measurement.SourcePath), out var row))
            row.Detail = Detail(measurement);

        TotalsText.Text = Totals();
    }

    /// <summary>The right-hand column: a file is just its size, a folder is what it holds.</summary>
    private static string Detail(DeleteMeasurement measurement)
    {
        if (!measurement.IsDirectory)
            return measurement.Incomplete ? "—" : ByteSizeFormatter.Format(measurement.Bytes);

        var contents = measurement.Files == 0
            ? "empty"
            : $"{measurement.Files:N0} file{(measurement.Files == 1 ? "" : "s")}, " +
              ByteSizeFormatter.Format(measurement.Bytes);
        return measurement.Incomplete ? contents + " *" : contents;
    }

    private string Totals()
    {
        var parts = new List<string>();
        if (_directories > 0) parts.Add($"{_directories:N0} folder{(_directories == 1 ? "" : "s")}");
        if (_files > 0) parts.Add($"{_files:N0} file{(_files == 1 ? "" : "s")}");

        var text = parts.Count == 0 ? "Nothing to remove" : string.Join(" and ", parts);
        if (_bytes > 0) text += $" — {ByteSizeFormatter.Format(_bytes)}";
        if (_measured < _rows.Count) text += " (still counting…)";
        else if (_incomplete) text += ". Some items could not be read, so this is a lower bound.";
        return text;
    }

    private void Delete_Click(object sender, RoutedEventArgs e) => DialogResult = true;
}

/// <summary>One line of the confirmation list.</summary>
/// <remarks>The icon loads the way the file list's does — inline when the shell can answer from the
/// registry, off-thread when it has to open the file — because a Ctrl+A over a folder of shortcuts
/// would otherwise stall the dialog before it appeared. Only realized rows ask, so a long list
/// costs no more than a short one.</remarks>
public sealed partial class DeleteItemRow : ObservableObject
{
    private ImageSource? _icon;
    private bool _iconLoaded;
    private bool _iconLoading;

    public DeleteItemRow(PlannedDelete item)
    {
        FullPath = item.SourcePath;
        IsDirectory = item.IsDirectory;
        Name = item.Name;
        Location = item.ParentPath;
    }

    public string FullPath { get; }

    public bool IsDirectory { get; }

    public string Name { get; }

    /// <summary>The folder it is being removed from — the part that stops two identically named
    /// files in a search result looking like the same item listed twice.</summary>
    public string Location { get; }

    /// <summary>What the survey found: a size, or what a folder holds. Empty until it gets there.</summary>
    [ObservableProperty]
    private string _detail = "";

    public ImageSource? Icon
    {
        get
        {
            if (_iconLoaded) return _icon;

            if (ShellIcons.IsPerFileIcon(FullPath, IsDirectory))
            {
                if (!_iconLoading)
                {
                    _iconLoading = true;
                    _ = LoadIconAsync();
                }
                return _icon;
            }

            _iconLoaded = true;
            return _icon = ShellIcons.GetIcon(FullPath, IsDirectory);
        }
    }

    private async Task LoadIconAsync()
    {
        var image = await Task.Run(() => ShellIcons.GetIcon(FullPath, IsDirectory));
        _icon = image;
        _iconLoaded = true;
        OnPropertyChanged(nameof(Icon));
    }
}
