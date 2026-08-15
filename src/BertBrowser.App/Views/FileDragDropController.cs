using System.Collections.Specialized;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.Interop;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>
/// Drag-and-drop for one pane's file list: dragging a selection onto a folder moves it there,
/// holding Ctrl copies instead. One instance per open tab, which is what makes dragging from one
/// pane into another work — the source reads its own list's selection, and the destination resolves
/// against its own tab's directory.
/// </summary>
/// <remarks>
/// Pressing on an already-selected row defers WPF's collapse-to-one-item selection until mouse-up,
/// so dragging a multi-selection actually drags all of it rather than the one row the mouse
/// happened to land on.
/// </remarks>
internal sealed class FileDragDropController
{
    private readonly ListView _list;
    private readonly DirectoryTabViewModel _tab;
    private readonly ShellViewModel _shell;
    private readonly DropPipeline _pipeline;

    private Point _pressOrigin;
    private FileItemViewModel? _dragCandidate;
    private FileItemViewModel? _deferredSelection;
    private bool _dragging;

    public static FileDragDropController Attach(
        ListView list, DirectoryTabViewModel tab, ShellViewModel shell) =>
        new(list, tab, shell);

    private FileDragDropController(ListView list, DirectoryTabViewModel tab, ShellViewModel shell)
    {
        _list = list;
        _tab = tab;
        _shell = shell;
        // Results report in the pane the user dropped into, not in whichever one happens to be
        // active when the transfer finishes.
        _pipeline = new DropPipeline(shell, message => tab.StatusText = message);

        list.PreviewMouseLeftButtonDown += OnListMouseDown;
        list.PreviewMouseMove += OnListMouseMove;
        list.PreviewMouseLeftButtonUp += OnListMouseUp;

        list.AllowDrop = true;
        list.DragOver += OnListDragOver;
        list.DragLeave += (_, _) => _pipeline.ClearHighlight();
        list.Drop += OnListDrop;
    }

    // --- Drag source ---

    private void OnListMouseDown(object sender, MouseButtonEventArgs e)
    {
        _dragCandidate = null;
        _deferredSelection = null;
        if (e.ClickCount > 1) return;

        if (VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
                is not { } container ||
            container.DataContext is not FileItemViewModel item)
            return;

        _pressOrigin = e.GetPosition(_list);
        _dragCandidate = item;

        // WPF reduces an extended selection to the clicked row on mouse-down, which would leave a
        // single item to drag. Hold that back until mouse-up, when we know it was a click.
        if (_list.SelectedItems.Count > 1 &&
            _list.SelectedItems.Contains(item) &&
            (Keyboard.Modifiers & (ModifierKeys.Control | ModifierKeys.Shift)) == 0)
        {
            _deferredSelection = item;
            container.Focus();
            e.Handled = true;
        }
    }

    private void OnListMouseMove(object sender, MouseEventArgs e)
    {
        if (_dragging || _dragCandidate is null) return;
        if (e.LeftButton != MouseButtonState.Pressed)
        {
            _dragCandidate = null;
            return;
        }

        var position = e.GetPosition(_list);
        if (Math.Abs(position.X - _pressOrigin.X) < SystemParameters.MinimumHorizontalDragDistance &&
            Math.Abs(position.Y - _pressOrigin.Y) < SystemParameters.MinimumVerticalDragDistance)
            return;

        StartDrag();
    }

    private void OnListMouseUp(object sender, MouseButtonEventArgs e)
    {
        // A press that turned out to be a plain click: apply the selection change we held back.
        if (_deferredSelection is { } item)
        {
            _list.SelectedItem = item;
            _deferredSelection = null;
        }
        _dragCandidate = null;
    }

    /// <summary>
    /// Builds the payload and runs the drag. The <see cref="DataObject"/> carries the private
    /// format <em>and</em> CF_HDROP: in-app targets read only the private one, so dropping between
    /// panes behaves exactly as it always has, while other applications see an ordinary file drop.
    /// </summary>
    private void StartDrag()
    {
        var paths = _list.SelectedItems.OfType<FileItemViewModel>()
            .Select(i => i.FullPath)
            .ToArray();
        if (paths.Length == 0) return;

        _dragging = true;
        _deferredSelection = null; // a drag consumes the deferred click

        using var session = DragSession.Begin();
        try
        {
            var data = new DataObject(DropPipeline.ItemsFormat, paths);

            // What makes this a drag source for Explorer, editors, browsers and mail clients.
            var files = new StringCollection();
            foreach (var path in paths) files.Add(path);
            data.SetFileDropList(files);

            // Ask for a copy. Between two folders on one volume Explorer would otherwise default to
            // a move, and dragging a file into another application should add it there rather than
            // quietly take it out of the folder being browsed. Shift still overrides this.
            DropEffectFormats.SetPreferred(data, DragDropEffects.Copy);

            var result = DragDrop.DoDragDrop(_list, data, DragDropEffects.Move | DragDropEffects.Copy);

            // Whether the originals are now ours to remove is the one genuinely dangerous question
            // here, so it is decided by a pure, tested rule rather than inline.
            var action = DragOutContract.Decide(
                session.HandledInApp,
                (DropEffect)(int)result,
                DropEffectFormats.LogicalPerformedOn(data),
                DropEffectFormats.PerformedOn(data));

            _ = FinishDragOutAsync(action, paths);
        }
        catch (COMException)
        {
            // The shell occasionally refuses to start a drag (e.g. another drag in flight).
        }
        finally
        {
            _dragging = false;
            _dragCandidate = null;
            _pipeline.InvalidateHoverCache();
            _pipeline.ClearHighlight();
        }
    }

    /// <summary>
    /// Acts on the drag's verdict. Nothing here deletes: a removal goes through the ordinary
    /// reversible delete, so an external application's say-so cannot reach past the delete
    /// planner's refusals and Ctrl+Z still puts everything back.
    /// </summary>
    private async Task FinishDragOutAsync(DragOutAction action, string[] paths)
    {
        try
        {
            switch (action)
            {
                case DragOutAction.RemoveSources:
                    await _shell.RemoveDraggedOutSourcesAsync(paths);
                    break;

                // An optimized move: the target relocated the items itself, so the folder they left
                // has changed underneath us even though there is nothing for us to remove.
                case DragOutAction.RefreshOnly:
                    await _shell.RefreshTabsShowingAsync(ParentsOf(paths));
                    break;
            }
        }
        catch (Exception ex)
        {
            // An unhandled exception on an async void continuation would take the process down.
            _tab.StatusText = $"Drag failed: {ex.Message}";
        }
    }

    private static IEnumerable<string> ParentsOf(IEnumerable<string> paths) =>
        paths.Select(Path.GetDirectoryName).OfType<string>().Distinct(StringComparer.OrdinalIgnoreCase);

    // --- Drop target ---

    private void OnListDragOver(object sender, DragEventArgs e) =>
        _pipeline.HandleDragOver(e, ListTarget(e), ListHighlight(e));

    private void OnListDrop(object sender, DragEventArgs e) => _pipeline.HandleDrop(e, ListTarget(e));

    /// <summary>A folder row takes the drop itself; anywhere else means the folder this pane is
    /// browsing — its own, not whichever pane happens to be active. Search results are a flattened
    /// view of many folders, so they have no single "here".</summary>
    private string? ListTarget(DragEventArgs e)
    {
        if (RowItem(e) is { IsDirectory: true } directory) return directory.FullPath;
        if (_tab.FileList.IsFlattened) return null;
        return _tab.CurrentPath.Length > 0 ? _tab.CurrentPath : null;
    }

    /// <summary>Only a folder row gets highlighted — a drop into the current folder has no one row
    /// to point at.</summary>
    private static DependencyObject? ListHighlight(DragEventArgs e) =>
        RowItem(e) is { IsDirectory: true }
            ? VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)
            : null;

    private static FileItemViewModel? RowItem(DragEventArgs e) =>
        VisualTreeUtil.FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)?.DataContext
            as FileItemViewModel;
}
