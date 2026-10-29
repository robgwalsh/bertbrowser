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
    private readonly SyncPreviewViewModel _view;

    private SyncPreviewDialog(SyncPreviewViewModel view)
    {
        InitializeComponent();
        DataContext = view;
        _view = view;
        HeadingText.Text = $"Make {Short(view.RightPath)} match {Short(view.LeftPath)}";

        view.Finished += OnFinished;
        Closed += (_, _) => view.Finished -= OnFinished;
    }

    /// <summary>The harness's way in, so a capture is of the same window the menu opens.</summary>
    internal static SyncPreviewDialog Create(SyncPreviewViewModel view) => new(view);

    /// <summary>
    /// Shows the preview, and stays up while the sync it agrees to runs.
    /// </summary>
    /// <remarks>
    /// It does not hand the answer back to be run elsewhere. Closing on Sync would leave the only
    /// account of a long operation — and the only way to stop it — in the status bar of a window
    /// the user has just been given a reason to look away from.
    /// </remarks>
    public static void Show(Window? owner, SyncPreviewViewModel view)
    {
        var dialog = new SyncPreviewDialog(view);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        dialog.ShowDialog();
    }

    private void OnFinished()
    {
        // Guarded because a cancelled run finishes too, and a dialog already closing must not be
        // told to close again.
        if (IsLoaded) DialogResult = true;
    }

    /// <summary>Just the folder's own name: the full paths are in the two panes behind this window,
    /// and a heading is not the place to re-read them.</summary>
    private static string Short(string path) =>
        Path.GetFileName(Path.TrimEndingDirectorySeparator(path)) is { Length: > 0 } name
            ? name
            : path;

    /// <summary>Starts the sync and hands the window over to it. Async void because it is an event
    /// handler; the modal message loop keeps pumping, so the bar and its Cancel stay live.</summary>
    private async void Run_Click(object sender, RoutedEventArgs e) => await _view.RunAsync();

    private void Cancel_Click(object sender, RoutedEventArgs e) => DialogResult = false;
}
