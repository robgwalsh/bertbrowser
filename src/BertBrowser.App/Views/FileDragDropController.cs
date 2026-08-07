using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Media3D;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>
/// Drag-and-drop transfers between the file list and the folder tree. Dragging a selection onto a
/// folder moves it there; holding Ctrl copies instead.
/// </summary>
/// <remarks>
/// Two things here exist purely to keep the operation honest:
/// <list type="bullet">
/// <item>The plan shown while hovering is cached per target, but the drop <em>re-plans from
/// scratch</em> before anything is written. The hover plan decides only whether the cursor says
/// "allowed"; it is never the plan that gets executed.</item>
/// <item>Pressing on an already-selected row defers WPF's collapse-to-one-item selection until
/// mouse-up, so dragging a multi-selection actually drags all of it rather than the one row the
/// mouse happened to land on.</item>
/// </list>
/// </remarks>
internal sealed class FileDragDropController
{
    /// <summary>Private clipboard format: in-app drops only. Deliberately not
    /// <see cref="DataFormats.FileDrop"/>, which would make this a drag source for Explorer.</summary>
    private const string ItemsFormat = "BertBrowser.FileItems";

    private readonly ListView _list;
    private readonly TreeView _tree;
    private readonly ShellViewModel _shell;
    private readonly Func<TransferPlan, ConflictResolution?> _askAboutConflicts;

    private Point _pressOrigin;
    private FileItemViewModel? _dragCandidate;
    private FileItemViewModel? _deferredSelection;
    private bool _dragging;

    private DependencyObject? _highlighted;

    // Hover plan cache: the sources are fixed for the duration of a drag, so only the target and
    // the verb can change.
    private string? _cachedTarget;
    private TransferVerb _cachedVerb;
    private bool _cachedAllowed;

    public static FileDragDropController Attach(
        ListView list, TreeView tree, ShellViewModel shell,
        Func<TransferPlan, ConflictResolution?> askAboutConflicts) =>
        new(list, tree, shell, askAboutConflicts);

    private FileDragDropController(
        ListView list, TreeView tree, ShellViewModel shell,
        Func<TransferPlan, ConflictResolution?> askAboutConflicts)
    {
        _list = list;
        _tree = tree;
        _shell = shell;
        _askAboutConflicts = askAboutConflicts;

        list.PreviewMouseLeftButtonDown += OnListMouseDown;
        list.PreviewMouseMove += OnListMouseMove;
        list.PreviewMouseLeftButtonUp += OnListMouseUp;

        list.AllowDrop = true;
        list.DragOver += OnListDragOver;
        list.DragLeave += (_, _) => ClearHighlight();
        list.Drop += OnListDrop;

        tree.AllowDrop = true;
        tree.DragOver += OnTreeDragOver;
        tree.DragLeave += (_, _) => ClearHighlight();
        tree.Drop += OnTreeDrop;
    }

    // --- Drag source ---

    private void OnListMouseDown(object sender, MouseButtonEventArgs e)
    {
        _dragCandidate = null;
        _deferredSelection = null;
        if (e.ClickCount > 1) return;

        if (FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject) is not { } container ||
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
            var data = new DataObject(ItemsFormat, paths);
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
            InvalidateHoverCache();
            ClearHighlight();
        }
    }

    // --- Drop target: file list ---

    private void OnListDragOver(object sender, DragEventArgs e) =>
        HandleDragOver(e, ListTarget(e), ListHighlight(e));

    private void OnListDrop(object sender, DragEventArgs e) => HandleDrop(e, ListTarget(e));

    /// <summary>A folder row takes the drop itself; anywhere else means the folder being browsed.
    /// Search results are a flattened view of many folders, so they have no single "here".</summary>
    private string? ListTarget(DragEventArgs e)
    {
        if (RowItem(e) is { IsDirectory: true } directory) return directory.FullPath;
        if (_shell.FileList.IsFlattened) return null;
        return _shell.CurrentPath.Length > 0 ? _shell.CurrentPath : null;
    }

    /// <summary>Only a folder row gets highlighted — a drop into the current folder has no one row
    /// to point at.</summary>
    private DependencyObject? ListHighlight(DragEventArgs e) =>
        RowItem(e) is { IsDirectory: true } ? FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject) : null;

    private static FileItemViewModel? RowItem(DragEventArgs e) =>
        FindAncestor<ListViewItem>(e.OriginalSource as DependencyObject)?.DataContext as FileItemViewModel;

    // --- Drop target: folder tree ---

    private void OnTreeDragOver(object sender, DragEventArgs e) =>
        HandleDragOver(e, TreeNode(e)?.FullPath, TreeHighlight(e));

    private void OnTreeDrop(object sender, DragEventArgs e) => HandleDrop(e, TreeNode(e)?.FullPath);

    private static DirectoryNodeViewModel? TreeNode(DragEventArgs e) =>
        FindAncestor<TreeViewItem>(e.OriginalSource as DependencyObject)?.DataContext
            is DirectoryNodeViewModel { FullPath.Length: > 0 } node
            ? node
            : null;

    private static DependencyObject? TreeHighlight(DragEventArgs e) =>
        TreeNode(e) is not null ? FindAncestor<TreeViewItem>(e.OriginalSource as DependencyObject) : null;

    // --- Shared drag-over / drop ---

    private void HandleDragOver(DragEventArgs e, string? destination, DependencyObject? highlight)
    {
        e.Handled = true;
        e.Effects = DragDropEffects.None;

        if (Sources(e) is not { Length: > 0 } sources || destination is null || _shell.IsTransferring)
        {
            ClearHighlight();
            return;
        }

        var verb = VerbFor(e.KeyStates);
        if (!IsAllowed(sources, destination, verb))
        {
            ClearHighlight();
            return;
        }

        e.Effects = verb == TransferVerb.Copy ? DragDropEffects.Copy : DragDropEffects.Move;
        SetHighlight(highlight);
    }

    /// <summary>Hover-time answer only: whether the cursor should say "you can drop here". The plan
    /// behind it is never the one executed.</summary>
    private bool IsAllowed(string[] sources, string destination, TransferVerb verb)
    {
        if (_cachedTarget == destination && _cachedVerb == verb) return _cachedAllowed;

        _cachedTarget = destination;
        _cachedVerb = verb;
        _cachedAllowed = _shell.PlanDrop(sources, destination, verb).HasWork;
        return _cachedAllowed;
    }

    private void InvalidateHoverCache()
    {
        _cachedTarget = null;
        _cachedAllowed = false;
    }

    private async void HandleDrop(DragEventArgs e, string? destination)
    {
        e.Handled = true;
        ClearHighlight();
        InvalidateHoverCache();

        if (Sources(e) is not { Length: > 0 } sources || destination is null) return;
        var verb = VerbFor(e.KeyStates);

        try
        {
            // Re-planned here against live disk state: the hover plan was only ever advisory.
            var plan = _shell.PlanDrop(sources, destination, verb);

            if (!plan.HasWork)
            {
                _shell.StatusText = plan.Problems.Count > 0
                    ? plan.Problems[0].Message
                    : "Nothing to move — those items are already there.";
                return;
            }

            IReadOnlyDictionary<string, ConflictResolution>? resolutions = null;
            if (plan.Conflicts.Count > 0)
            {
                if (_askAboutConflicts(plan) is not { } resolution)
                {
                    _shell.StatusText = "Drop cancelled.";
                    return;
                }
                resolutions = plan.Transfers.ToDictionary(
                    t => BertBrowser.Core.Paths.PathKey.Canonicalize(t.SourcePath), _ => resolution);
            }

            await _shell.ExecuteDropAsync(plan, resolutions);
        }
        catch (Exception ex)
        {
            // An unhandled exception in an async void handler would take the process down.
            _shell.StatusText = $"Drop failed: {ex.Message}";
        }
    }

    private static string[]? Sources(DragEventArgs e) =>
        e.Data.GetDataPresent(ItemsFormat) ? e.Data.GetData(ItemsFormat) as string[] : null;

    private static TransferVerb VerbFor(DragDropKeyStates keys) =>
        (keys & DragDropKeyStates.ControlKey) != 0 ? TransferVerb.Copy : TransferVerb.Move;

    // --- Drop-target highlight ---

    private void SetHighlight(DependencyObject? target)
    {
        if (ReferenceEquals(_highlighted, target)) return;
        ClearHighlight();
        if (target is null) return;
        DropTarget.SetIsActive(target, true);
        _highlighted = target;
    }

    private void ClearHighlight()
    {
        if (_highlighted is null) return;
        DropTarget.SetIsActive(_highlighted, false);
        _highlighted = null;
    }

    private static T? FindAncestor<T>(DependencyObject? d) where T : DependencyObject
    {
        while (d is not null and not T)
            d = d is Visual or Visual3D ? VisualTreeHelper.GetParent(d) : LogicalTreeHelper.GetParent(d);
        return d as T;
    }
}

/// <summary>Marks the container currently under a drag, so the row styles can highlight it.</summary>
public static class DropTarget
{
    public static readonly DependencyProperty IsActiveProperty = DependencyProperty.RegisterAttached(
        "IsActive", typeof(bool), typeof(DropTarget), new PropertyMetadata(false));

    public static void SetIsActive(DependencyObject element, bool value) =>
        element.SetValue(IsActiveProperty, value);

    public static bool GetIsActive(DependencyObject element) =>
        (bool)element.GetValue(IsActiveProperty);
}
