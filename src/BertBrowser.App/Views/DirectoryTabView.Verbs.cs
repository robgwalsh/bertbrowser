using System.Windows.Controls;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Commands;
using BertBrowser.Core.Services.NewItem;

namespace BertBrowser.App.Views;

/// <summary>
/// The one way into this tab's file verbs. The right-click menu, a shortcut and the command palette
/// all arrive here, and each verb is checked against <see cref="FileVerbRules"/> on the way in —
/// the menu greying an item out is a courtesy, this is the guard.
/// </summary>
/// <remarks>
/// The verbs stay in the view rather than moving to the view model because nearly every one of
/// them opens a dialog owned by this window and reads the list's own selection order.
/// </remarks>
public partial class DirectoryTabView
{
    /// <summary>What the rules need to know about this tab right now.</summary>
    internal FileVerbSnapshot VerbSnapshot(IReadOnlyList<FileItemViewModel>? selected = null)
    {
        var selection = selected ?? SelectedFileItems();
        var inArchive = Tab.FileList.IsInsideArchive;

        return new FileVerbSnapshot(
            Count: selection.Count,
            Files: selection.Count(i => !i.IsDirectory),
            InArchive: inArchive,
            IsSearchResult: Tab.FileList.IsSearchResult,
            HasFolder: Tab.CurrentPath.Length > 0,
            Comparing: _shell.CompareSession is not null,
            SingleIsArchive: selection is [{ IsDirectory: false } one] && ArchiveFormats.IsArchiveName(one.Name),
            AllBookmarked: selection.Count > 0 && selection.All(i => _shell.Bookmarks.IsBookmarked(i.FullPath)),
            ArchiveLocked: Tab.FileList.IsArchiveLocked,
            ClipboardHasFiles: FileClipboard.HasFiles,
            CanRunElevated: () => selection is [var only] &&
                Interop.RunAsVerbRegistry.CanRunElevated(only.FullPath, only.IsDirectory, inArchive));
    }

    internal FileVerbState VerbState(FileVerb verb) => FileVerbRules.For(verb, VerbSnapshot());

    /// <summary>Runs a verb if the rules allow it here. False when they do not — the caller has
    /// nothing to report, since whoever offered the verb had the reason to show.</summary>
    internal bool RunVerb(FileVerb verb)
    {
        if (!VerbState(verb).Enabled) return false;

        switch (verb)
        {
            case FileVerb.Open:
                if (FileListView.SelectedItem is FileItemViewModel opened)
                    Tab.OpenItemCommand.Execute(opened);
                break;
            case FileVerb.RunAsAdmin:
                if (FileListView.SelectedItem is FileItemViewModel elevated)
                    Tab.Open(elevated, elevated: true);
                break;
            case FileVerb.OpenInNewTab:
                foreach (var folder in SelectedFolders())
                    _shell.OpenInNewTab(folder.FullPath);
                break;
            case FileVerb.OpenInNewPane:
                OpenSelectedFolderInPane(SplitOrientation.Vertical);
                break;
            case FileVerb.OpenInTerminal:
                if (LaunchTarget() is { } terminal)
                    _shell.OpenInTerminal(terminal.FullPath, terminal.IsDirectory);
                break;
            case FileVerb.OpenInVSCode:
                if (LaunchTarget() is { } code)
                    _shell.OpenInVSCode(code.FullPath, code.IsDirectory);
                break;
            case FileVerb.DiskUsage:
                _shell.OpenDiskUsage(SelectedFolderOrCurrent());
                break;
            case FileVerb.Duplicates:
                _shell.OpenDuplicates(SelectedFolderOrCurrent());
                break;
            case FileVerb.Changes:
                _shell.OpenChanges(SelectedFolderOrCurrent());
                break;
            // Deliberately routed through the shell's command rather than acting on the selection:
            // the pair is decided by which panes are on screen, and a selected folder has nothing
            // to do with it.
            case FileVerb.ComparePanes:
                _shell.CompareWithOtherPaneCommand.Execute(null);
                break;
            case FileVerb.Compress:
                _ = CompressAsync();
                break;
            case FileVerb.ExtractHere:
                _ = ExtractAsync(askWhere: false);
                break;
            case FileVerb.ExtractTo:
                _ = ExtractAsync(askWhere: true);
                break;
            case FileVerb.CopyPath:
                _shell.CopyPathsCommand.Execute(SelectedFileItems());
                break;
            case FileVerb.CopyName:
                _shell.CopyNamesCommand.Execute(SelectedFileItems());
                break;
            case FileVerb.CompareFiles:
                CompareSelectedFiles();
                break;
            case FileVerb.SettleByContent:
                SettleSelectionByContent();
                break;
            case FileVerb.VerifyChecksums:
                VerifyChecksums();
                break;
            case FileVerb.Checksum:
                ChecksumSelection();
                break;
            case FileVerb.Cut:
                _shell.CutSelectionCommand.Execute(SelectedFileItems());
                break;
            case FileVerb.Copy:
                _shell.CopySelectionCommand.Execute(SelectedFileItems());
                break;
            case FileVerb.Paste:
                _shell.PasteCommand.Execute(null);
                break;
            case FileVerb.Rename:
                _ = RenameSelectionAsync();
                break;
            case FileVerb.Delete:
                _ = DeleteSelectionAsync(permanent: false);
                break;
            case FileVerb.DeletePermanently:
                _ = DeleteSelectionAsync(permanent: true);
                break;
            case FileVerb.Bookmark:
                _ = _shell.ToggleBookmarksAsync(
                    SelectedFileItems().Select(i => (i.FullPath, i.IsDirectory)).ToList());
                break;
            case FileVerb.Properties:
                ShowProperties(SelectedFileItems());
                break;
            case FileVerb.UnlockArchive:
                _ = UnlockAsync();
                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(verb), verb, "Not a verb that runs by itself.");
        }

        return true;
    }

    /// <summary>New, which is one rule and several things to make.</summary>
    internal bool RunNew(NewItemKind kind, NewFileTemplate? template = null)
    {
        if (!VerbState(FileVerb.New).Enabled) return false;
        _ = CreateInCurrentFolderAsync(kind, template);
        return true;
    }

    /// <summary>"Open in new pane", which is one rule and two directions.</summary>
    internal bool RunOpenInPane(SplitOrientation orientation)
    {
        if (!VerbState(FileVerb.OpenInNewPane).Enabled) return false;
        OpenSelectedFolderInPane(orientation);
        return true;
    }

    /// <summary>A selected folder, or — with nothing selected — the folder being shown.</summary>
    private string? SelectedFolderOrCurrent()
    {
        var target = FileListView.SelectedItem is FileItemViewModel { IsDirectory: true } item
            ? item.FullPath
            : Tab.CurrentPath;

        return target is { Length: > 0 } ? target : null;
    }

    /// <summary>Sets one menu item from its verb's rule: on or off, what it says, and gone
    /// altogether where the rule says there is nothing to offer.</summary>
    private static void Offer(
        MenuItem item, FileVerb verb, FileVerbSnapshot snapshot, IReadOnlySet<string> hidden)
    {
        var state = FileVerbRules.For(verb, snapshot);
        item.IsEnabled = state.Enabled;
        item.Header = state.Header;
        if (!state.Visible) BuiltInMenu.Show(item, show: false, hidden);
    }
}
