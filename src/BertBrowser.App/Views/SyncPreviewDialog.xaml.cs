using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Compare;

namespace BertBrowser.App.Views;

/// <summary>
/// What a sync is about to do, before it does any of it.
/// </summary>
/// <remarks>
/// It shows the same <see cref="SyncPreview"/> the run is handed, so what is listed is what
/// happens. The rows are blocked Copy, Replace, Delete, which puts the destructive part of the run
/// at the bottom as its own visible thing rather than mixed into a list of copies — and the
/// deletions are not there at all until they are asked for.
/// </remarks>
public partial class SyncPreviewDialog : ThemedWindow
{
    private SyncPreviewDialog(SyncPreviewViewModel view)
    {
        InitializeComponent();
        DataContext = view;
        HeadingText.Text = $"Make {Short(view.RightPath)} match {Short(view.LeftPath)}";
    }

    /// <summary>The harness's way in, so a capture is of the same window the menu opens.</summary>
    internal static SyncPreviewDialog Create(SyncPreviewViewModel view) => new(view);

    /// <summary>Shows the preview and returns what to run, or null when the user backed out.</summary>
    public static SyncPreview? Confirm(Window? owner, SyncPreviewViewModel view)
    {
        var dialog = new SyncPreviewDialog(view);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true ? view.Result : null;
    }

    /// <summary>Just the folder's own name: the full paths are in the two panes behind this window,
    /// and a heading is not the place to re-read them.</summary>
    private static string Short(string path) =>
        Path.GetFileName(Path.TrimEndingDirectorySeparator(path)) is { Length: > 0 } name
            ? name
            : path;

    private void Run_Click(object sender, RoutedEventArgs e) => DialogResult = true;

    private void Cancel_Click(object sender, RoutedEventArgs e) => DialogResult = false;
}
