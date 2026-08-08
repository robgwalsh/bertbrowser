using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

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

    private void StartDrag()
    {
        var paths = _list.SelectedItems.OfType<FileItemViewModel>()
            .Select(i => i.FullPath)
            .ToArray();
        if (paths.Length == 0) return;

        _dragging = true;
        _deferredSelection = null; // a drag consumes the deferred click
        try
        {
            var data = new DataObject(DropPipeline.ItemsFormat, paths);
            DragDrop.DoDragDrop(_list, data, DragDropEffects.Move | DragDropEffects.Copy);
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
