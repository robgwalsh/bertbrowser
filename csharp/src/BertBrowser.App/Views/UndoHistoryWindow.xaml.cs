using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services;

namespace BertBrowser.App.Views;

/// <summary>
/// The undo history: every operation this session can still take back or do again, newest first,
/// with a divider where the cursor is.
/// </summary>
/// <remarks>
/// Modeless, and bound to the shell itself, so its Undo and Redo are the same commands as Ctrl+Z and
/// Ctrl+Y and its rows update as operations happen behind it. It runs nothing of its own: every
/// action is a line into the shell.
/// </remarks>
public partial class UndoHistoryWindow : ThemedWindow
{
    private readonly ShellViewModel _shell;
    private readonly IUserConfirm _confirm;
    private readonly Action<string, bool> _reveal;
    private readonly Action _openSettings;

    /// <param name="reveal">Takes a path and whether it is a directory, and puts the app there —
    /// supplied rather than reached for, as the change timeline's is.</param>
    /// <param name="openSettings">Opens Settings on the History page, where the limits are.</param>
    private UndoHistoryWindow(
        ShellViewModel shell, IUserConfirm confirm, Action<string, bool> reveal, Action openSettings)
    {
        InitializeComponent();
        _shell = shell;
        _confirm = confirm;
        _reveal = reveal;
        _openSettings = openSettings;
        DataContext = shell;
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static UndoHistoryWindow Create(
        ShellViewModel shell, IUserConfirm confirm, Action<string, bool> reveal, Action openSettings) =>
        new(shell, confirm, reveal, openSettings);

    /// <summary>
    /// Opens where the row's result can be seen: its destination while it is in effect, where it
    /// came from once undone. A path that has gone since falls back to its folder.
    /// </summary>
    private void Reveal_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not UndoEntryViewModel { RevealPath: { } path }) return;

        if (Directory.Exists(path)) _reveal(path, true);
        else if (File.Exists(path)) _reveal(path, false);
        else if (Path.GetDirectoryName(path) is { Length: > 0 } folder && Directory.Exists(folder)) _reveal(folder, true);
    }

    private void Limits_Click(object sender, RoutedEventArgs e) => _openSettings();

    /// <summary>
    /// Commits everything the history holds, after saying how much that is. Asked first because it
    /// is the one action here that cannot be taken back.
    /// </summary>
    private void Clear_Click(object sender, RoutedEventArgs e) => ConfirmAndClear(_shell, _confirm);

    /// <summary>The one Clear, for this window's button and the command alike.</summary>
    internal static void ConfirmAndClear(ShellViewModel shell, IUserConfirm confirm)
    {
        var held = shell.History.Held;
        var what = held.Bytes > 0
            ? $" BertBrowser is holding {(held.Complete ? "" : "at least ")}{ByteSizeFormatter.Format(held.Bytes)} " +
              "of replaced and deleted files for them, which will be removed for good."
            : "";

        if (confirm.Ask(
                $"Clear the undo history? None of these actions can be undone or redone afterwards.{what} " +
                "Items sent to the Recycle Bin stay there.",
                "Clear undo history",
                "Clear history"))
        {
            _ = shell.ClearUndoHistoryAsync();
        }
    }
}
