using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Commands;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>
/// The tab's actions that are not file verbs: changing what is selected, editing the address, and
/// the transfers that start from a command rather than from a drop. Reached through the command
/// registry, like the verbs — there is no menu or button for most of them.
/// </summary>
public partial class DirectoryTabView
{
    // --- Selection ---

    /// <summary>True while a command is replacing the selection, so the mirror runs once at the
    /// end rather than from inside the change.</summary>
    private bool _bulkSelecting;

    internal int RowCount => FileListView.Items.Count;

    internal void SelectAllRows() => FileListView.SelectAll();

    internal void SelectNoRows() => FileListView.UnselectAll();

    internal void InvertSelection()
    {
        var selected = FileListView.SelectedItems.Cast<FileItemViewModel>().ToHashSet();
        SetSelection(Rows().Where(row => !selected.Contains(row)));
    }

    /// <summary>Selects exactly the rows <paramref name="wanted"/> picks out of the listing.</summary>
    internal void SelectWhere(Func<FileItemViewModel, bool> wanted) => SetSelection(Rows().Where(wanted));

    /// <summary>
    /// "Select by pattern" and "deselect by pattern": asks for one, in the search box's language,
    /// and adds the matching rows to the selection or takes them out of it.
    /// </summary>
    internal void SelectByPattern(bool select)
    {
        var rows = Rows().ToList();
        if (rows.Count == 0) return;

        var picked = SelectPatternDialog.Show(Window.GetWindow(this), rows.Select(AsSelectionRow).ToList(), select);
        if (picked is null) return;

        var chosen = picked.Select(i => rows[i]).ToHashSet();
        var selection = FileListView.SelectedItems.Cast<FileItemViewModel>().ToHashSet();
        if (select) selection.UnionWith(chosen);
        else selection.ExceptWith(chosen);

        // In list order, so the first selected row is the topmost one — which is the row the
        // preview and "Open" act on.
        SetSelection(rows.Where(selection.Contains));
        FileListView.Focus();
    }

    private IEnumerable<FileItemViewModel> Rows() => FileListView.Items.OfType<FileItemViewModel>();

    private static SelectionRow AsSelectionRow(FileItemViewModel row) => new(
        row.Name, row.FullPath, row.IsDirectory, row.SizeBytes ?? 0, row.ModifiedUtc, row.CreatedUtc, row.Attributes);

    private void SetSelection(IEnumerable<FileItemViewModel> rows)
    {
        _bulkSelecting = true;
        try
        {
            // One selection change, not one per row — see BulkSelectListView.
            FileListView.SelectOnly(rows.ToList());
        }
        finally
        {
            _bulkSelecting = false;
        }

        MirrorSelection();
    }

    // --- Address bar ---

    /// <summary>Swaps the breadcrumb for the editable path, with the path selected. What clicking
    /// the bar's empty space has always done, now reachable from the keyboard.</summary>
    internal void EditAddress()
    {
        // Pressed again while a path is being typed, it must not throw the typing away.
        if (PathBox.Visibility == Visibility.Visible)
        {
            PathBox.Focus();
            return;
        }

        PathBox.Text = Tab.CurrentPath;
        BreadcrumbScroller.Visibility = Visibility.Collapsed;
        PathBox.Visibility = Visibility.Visible;
        PathBox.Focus();
        PathBox.SelectAll();
    }

    // --- Columns and the preview ---

    /// <summary>The whole property system, hung under the address bar: a command has no pointer
    /// to open it beside, and the bar is the nearest thing to the header strip that is always there.</summary>
    internal void ShowColumnPickerFromCommand() => ColumnAddPopup.Show(
        BreadcrumbScroller,
        () => Tab.FileList.ColumnLayout,
        id => Tab.FileList.ColumnLayout = Core.Services.Columns.ColumnLayoutRules.Toggle(Tab.FileList.ColumnLayout, id, on: true),
        atMouse: false);

    internal void UseColumnsForNewTabs() => SaveColumnsAsDefault();

    internal void TogglePreviewPlayback() => _previewPane?.TogglePlayback();

    internal void TogglePreviewFullScreen() => _previewPane?.ToggleFullScreen();

    // --- Transfers that start from a command ---

    /// <summary>The selection's paths in the order the list shows them, which is the order a
    /// transfer's progress and its conflict dialog read in.</summary>
    internal IReadOnlyList<string> SelectedPathsInListOrder()
    {
        var selection = SelectedFileItems().ToHashSet();
        return Rows().Where(selection.Contains).Select(row => row.FullPath).ToList();
    }

    /// <summary>The one selected folder, when that is exactly what is selected.</summary>
    internal string? SelectedSingleFolder() =>
        SelectedFileItems() is [{ IsDirectory: true } only] ? only.FullPath : null;

    /// <summary>Asks for a folder and copies or moves the selection into it.</summary>
    internal void TransferSelectionToChosenFolder(TransferVerb verb)
    {
        var sources = SelectedPathsInListOrder();
        if (sources.Count == 0) return;

        var dialog = new Microsoft.Win32.OpenFolderDialog
        {
            Title = verb == TransferVerb.Move ? "Move to folder" : "Copy to folder",
            InitialDirectory = Tab.CurrentPath is { Length: > 0 } here && Directory.Exists(here) ? here : null,
            Multiselect = false,
        };

        if (dialog.ShowDialog(Window.GetWindow(this)) == true)
            _ = _shell.TransferToAsync(sources, dialog.FolderName, verb);
    }
}
