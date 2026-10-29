using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// "What do I have two of?" — files that are byte-for-byte identical, grouped, with the copies the
/// user marks handed to the app's ordinary reversible delete.
/// </summary>
/// <remarks>
/// Modeless on purpose, for the reason the disk-usage view is: following what it says means going
/// to a folder in a tab behind it. It also outlives a scan that can run for minutes, which a dialog
/// could not.
/// </remarks>
public partial class DuplicatesWindow : ThemedWindow
{
    /// <remarks>
    /// This window does <em>not</em> dispose it. The view model outlives any one window — the
    /// harness photographs a finished scan by wrapping the same one — so whoever made it owns it,
    /// the way <c>MainWindow</c> owns the transfer progress view model. A window that disposed what
    /// it was handed would silently kill a scan the caller still wanted.
    /// </remarks>
    private readonly DuplicatesViewModel _vm;
    private readonly Action<string, bool> _reveal;

    /// <param name="reveal">Takes a path and whether it is a directory, and puts the app there —
    /// supplied rather than reached for, so this window knows nothing about the shell.</param>
    public DuplicatesWindow(DuplicatesViewModel vm, Action<string, bool> reveal)
    {
        InitializeComponent();
        _vm = vm;
        _reveal = reveal;
        DataContext = vm;
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static DuplicatesWindow Create(DuplicatesViewModel vm, Action<string, bool> reveal) =>
        new(vm, reveal);

    /// <summary>Points the window at <paramref name="path"/> (null being "This PC") without
    /// starting anything: a whole-PC scan reads real files, so it waits to be asked.</summary>
    public void Load(string? path) => _vm.RootPath = path;

    /// <summary>
    /// Double-clicking a copy shows it where it lives, which is how you tell two identical files
    /// apart before deciding which to keep. Single clicks are left alone — the checkbox is what
    /// acts, and opening a folder on the way to ticking a box would be maddening.
    /// </summary>
    private void Copy_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ChangedButton != MouseButton.Left || e.ClickCount != 2) return;
        if (((FrameworkElement)sender).DataContext is not DuplicateFileViewModel copy) return;

        _reveal(copy.FullPath, false);
        e.Handled = true;
    }
}
