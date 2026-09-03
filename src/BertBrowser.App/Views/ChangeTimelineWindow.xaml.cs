using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// "What changed here?" — the change log the index helper keeps, filtered to a folder and a range,
/// refreshing as the changes arrive. Nothing here reads the journal; see the view model.
/// </summary>
/// <remarks>
/// Modeless for the reason the disk-usage view is, and one more: the thing causing the changes —
/// an installer, a build — is usually still running behind it.
/// </remarks>
public partial class ChangeTimelineWindow : ThemedWindow
{
    private readonly ChangeTimelineViewModel _vm;
    private readonly Action<string, bool> _reveal;
    private readonly Action _openSettings;

    /// <param name="reveal">Takes a path and whether it is a directory, and puts the app there —
    /// supplied rather than reached for, so this window knows nothing about the shell.</param>
    /// <param name="openSettings">Opens the settings dialog on the History page, where the
    /// recording switch is. Supplied for the same reason.</param>
    public ChangeTimelineWindow(ChangeTimelineViewModel vm, Action<string, bool> reveal, Action openSettings)
    {
        InitializeComponent();
        _vm = vm;
        _reveal = reveal;
        _openSettings = openSettings;
        DataContext = vm;

        // The view model holds a timer and subscriptions to the index service, both of which
        // outlive a window that is modeless.
        Closed += (_, _) => _vm.Dispose();
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static ChangeTimelineWindow Create(ChangeTimelineViewModel vm, Action<string, bool> reveal, Action openSettings) =>
        new(vm, reveal, openSettings);

    /// <summary>Points the window at <paramref name="path"/> (null being "This PC").</summary>
    public void Load(string? path) => _ = _vm.LoadAsync(path);

    /// <summary>
    /// Shows the row where it lives. A deleted item has nowhere to be shown, so its folder is shown
    /// instead — which is also the answer for anything that has gone since the row was written.
    /// </summary>
    private void Row_DoubleClick(object sender, RoutedEventArgs e)
    {
        if (RowList.SelectedItem is not ChangeEventViewModel row) return;

        if (!row.IsDeleted && (row.IsDirectory ? Directory.Exists(row.FullPath) : File.Exists(row.FullPath)))
            _reveal(row.FullPath, row.IsDirectory);
        else if (row.Folder.Length > 0 && Directory.Exists(row.Folder))
            _reveal(row.Folder, true);
    }

    private void OpenSettings_Click(object sender, RoutedEventArgs e) => _openSettings();

    private void Copy_CanExecute(object sender, CanExecuteRoutedEventArgs e) =>
        e.CanExecute = RowList.SelectedItems.Count > 0;

    /// <summary>Ctrl+C: the selected rows as text — the shape a "what did the installer touch"
    /// question is usually answered in, pasted somewhere.</summary>
    private void Copy_Executed(object sender, ExecutedRoutedEventArgs e) =>
        _vm.CopyRows(RowList.SelectedItems.OfType<ChangeEventViewModel>());
}
