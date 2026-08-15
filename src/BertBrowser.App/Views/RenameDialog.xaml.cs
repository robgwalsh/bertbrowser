using System.Text;
using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks for the new name. One item is renamed to exactly what is typed; several are numbered from
/// it — "Holiday" over three photos gives "Holiday 1", "Holiday 2", "Holiday 3", each keeping its
/// own extension.
/// </summary>
/// <remarks>
/// The dialog re-plans on every keystroke rather than validating the text on its own, so what it
/// previews is what the rename will do, and a name that is already taken is refused here — before
/// anything is written — instead of failing halfway through a batch.
/// </remarks>
public partial class RenameDialog : ThemedWindow
{
    /// <summary>How many "old → new" lines to list before summarising the rest.</summary>
    private const int PreviewLines = 6;

    private readonly IReadOnlyList<RenameSource> _sources;
    private readonly Func<IReadOnlyList<RenameSource>, string, RenamePlan> _planner;

    private RenamePlan _plan = RenamePlan.Empty;

    private RenameDialog(
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, string, RenamePlan> planner)
    {
        InitializeComponent();
        _sources = sources;
        _planner = planner;

        PromptText.Text = sources.Count == 1
            ? "New name:"
            : $"New name for {sources.Count:N0} items — they will be numbered:";

        NameBox.Text = RenamePattern.SuggestFor(sources);
        Replan(); // the suggestion itself may be a no-op, which must not leave Rename enabled
        Loaded += (_, _) =>
        {
            NameBox.Focus();
            // Select the part a rename usually replaces, leaving a file's extension alone.
            NameBox.Select(0, sources.Count == 1 ? RenamePattern.BaseNameLength(sources[0]) : NameBox.Text.Length);
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. Nothing in the app uses it: <see cref="Show"/> is the only way in from the
    /// interface, and it goes through this same constructor.</summary>
    internal static RenameDialog Create(
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, string, RenamePlan> planner) => new(sources, planner);

    /// <summary>Shows the dialog and returns the plan to carry out, or null if it was cancelled or
    /// there was nothing to do.</summary>
    public static RenamePlan? Show(
        Window? owner,
        IReadOnlyList<RenameSource> sources,
        Func<IReadOnlyList<RenameSource>, string, RenamePlan> planner)
    {
        if (sources.Count == 0) return null;

        var dialog = new RenameDialog(sources, planner);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? dialog._plan : null;
    }

    private void NameBox_TextChanged(object sender, TextChangedEventArgs e) => Replan();

    private void Replan()
    {
        _plan = _planner(_sources, NameBox.Text);

        // Any refusal blocks the whole rename: a batch that silently skips some of its items is
        // worse than one that says what is wrong while the name can still be changed.
        var problem = _plan.Rejected.FirstOrDefault();
        ProblemText.Text = problem?.Message ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;

        PreviewText.Text = Preview();
        PreviewText.Visibility = PreviewText.Text.Length == 0 ? Visibility.Collapsed : Visibility.Visible;

        OkButton.IsEnabled = problem is null && _plan.HasWork;
    }

    /// <summary>"old → new" for the first few items, then a count for the rest. A single item shows
    /// nothing: the text box already says what it will be called.</summary>
    private string Preview()
    {
        if (_sources.Count < 2) return "";

        var work = _plan.Renames;
        if (work.Count == 0) return "";

        var text = new StringBuilder();
        foreach (var rename in work.Take(PreviewLines))
        {
            if (text.Length > 0) text.Append('\n');
            text.Append(rename.SourceName).Append("  →  ").Append(rename.TargetName);
        }
        if (work.Count > PreviewLines)
            text.Append($"\n…and {work.Count - PreviewLines:N0} more");
        return text.ToString();
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Replan(); // disk may have changed while the dialog sat open
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
