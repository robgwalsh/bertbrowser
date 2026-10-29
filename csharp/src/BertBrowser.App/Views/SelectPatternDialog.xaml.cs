using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.Commands;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks for the pattern "select by pattern" and "deselect by pattern" pick rows with, and says as
/// it is typed how many rows that is — so the answer is known before anything is selected.
/// </summary>
public partial class SelectPatternDialog : ThemedWindow
{
    private readonly IReadOnlyList<SelectionRow> _rows;
    private IReadOnlyList<int>? _result;

    private SelectPatternDialog(IReadOnlyList<SelectionRow> rows, bool select, string seed)
    {
        InitializeComponent();
        _rows = rows;

        Title = select ? "Select by pattern" : "Deselect by pattern";
        PromptText.Text = select ? "Select the items here that match" : "Deselect the items here that match";
        OkButton.Content = select ? "Select" : "Deselect";
        PatternBox.Text = seed;

        Evaluate();
        Loaded += (_, _) =>
        {
            PatternBox.Focus();
            PatternBox.SelectAll();
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and photograph.</summary>
    internal static SelectPatternDialog Create(IReadOnlyList<SelectionRow> rows, bool select, string seed = "") =>
        new(rows, select, seed);

    /// <summary>The positions in <paramref name="rows"/> the pattern picked, or null on Cancel.</summary>
    public static IReadOnlyList<int>? Show(Window? owner, IReadOnlyList<SelectionRow> rows, bool select, string seed = "")
    {
        var dialog = new SelectPatternDialog(rows, select, seed) { Owner = owner };
        return dialog.ShowDialog() == true ? dialog._result : null;
    }

    private void Pattern_TextChanged(object sender, TextChangedEventArgs e) => Evaluate();

    private SelectionMatch Evaluate()
    {
        var match = SelectionPattern.Match(PatternBox.Text, _rows);

        if (match.Problem is { } problem)
        {
            ResultText.Text = problem;
            ResultText.SetResourceReference(TextBlock.ForegroundProperty, "Theme.Error.Foreground");
        }
        else
        {
            ResultText.Text = string.IsNullOrWhiteSpace(PatternBox.Text)
                ? $"{_rows.Count:N0} item(s) here."
                : $"{match.Indexes.Count:N0} of {_rows.Count:N0} item(s) match.";
            ResultText.SetResourceReference(TextBlock.ForegroundProperty, "Theme.Text.Secondary");
        }

        OkButton.IsEnabled = match.Problem is null && match.Indexes.Count > 0;
        return match;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        var match = Evaluate();
        if (match.Problem is not null || match.Indexes.Count == 0) return;

        _result = match.Indexes;
        DialogResult = true;
    }
}
