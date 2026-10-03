namespace BertBrowser.Core.Services.Commands;

/// <summary>The things a file list's right-click menu offers, each of which is also a command.</summary>
public enum FileVerb
{
    New,
    Open,
    RunAsAdmin,
    OpenInNewTab,
    OpenInNewPane,
    OpenInTerminal,
    OpenInVSCode,
    DiskUsage,
    Duplicates,
    Changes,
    ComparePanes,
    Compress,
    ExtractHere,
    ExtractTo,
    CopyPath,
    CopyName,
    CompareFiles,
    SettleByContent,
    VerifyChecksums,
    Checksum,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    DeletePermanently,
    Bookmark,
    Properties,
    UnlockArchive,
}

/// <summary>
/// What a file verb needs to know about the tab it would act on. Counts rather than items, so the
/// rules can be tested without a list; the two answers that cost something to get — the clipboard
/// and the registry — are asked for only by the verbs that need them.
/// </summary>
/// <param name="Count">How many rows are selected.</param>
/// <param name="Files">How many of those are files rather than folders.</param>
/// <param name="HasFolder">The tab is showing somewhere with a path — not This PC.</param>
/// <param name="SingleIsArchive">Exactly one row is selected and it is an archive file.</param>
/// <param name="CanRunElevated">Whether the one selected item has anything to elevate.</param>
public sealed record FileVerbSnapshot(
    int Count,
    int Files,
    bool InArchive,
    bool IsSearchResult,
    bool HasFolder,
    bool Comparing = false,
    bool SingleIsArchive = false,
    bool AllBookmarked = false,
    bool ArchiveLocked = false,
    Func<bool>? ClipboardHasFiles = null,
    Func<bool>? CanRunElevated = null)
{
    public int Folders => Count - Files;
}

/// <summary>Whether a verb is offered, whether it can run, what it is called for this selection,
/// and — when it cannot run — why, in words fit to show beside it.</summary>
public readonly record struct FileVerbState(bool Visible, bool Enabled, string Header, string? Reason)
{
    public static FileVerbState On(string header) => new(true, true, header, null);

    public static FileVerbState Off(string header, string reason) => new(true, false, header, reason);
}

/// <summary>
/// When each file verb is available. These rules used to exist only as lines setting
/// <c>IsEnabled</c> on menu items, which made the menu the single thing standing between a verb and
/// a selection it must not touch. They are here so that the menu, a shortcut and the command
/// palette all get one answer — a keybinding must not be able to route around a menu, and nor must
/// a palette.
/// </summary>
public static class FileVerbRules
{
    private const string NothingSelected = "Nothing is selected.";
    private const string InsideArchive = "Not available inside an archive.";

    public static FileVerbState For(FileVerb verb, FileVerbSnapshot s)
    {
        // Inside an archive nothing has a path an executor can act on, so everything that writes to
        // disk by path is off. The same guard the flattened-search rules use, for the same reason,
        // and the dangerous ones are guarded at the service too.
        var inArchive = s.InArchive;

        switch (verb)
        {
            // Copy is off inside a container with Cut, and that one is worth stating: Ctrl+C is a
            // promise to produce bytes *later*, and an entry cannot keep it — the container may
            // have been rewritten by the time anyone pastes. Explorer keeps the promise by
            // extracting to temp on the keystroke, which writes gigabytes for a keypress. Extract
            // is the honest verb and it gets its own menu item.
            case FileVerb.Cut:
            case FileVerb.Copy:
            {
                var header = verb == FileVerb.Cut ? "Cut" : "Copy";
                if (s.Count == 0) return FileVerbState.Off(header, NothingSelected);
                return inArchive
                    ? FileVerbState.Off(header, "Not available inside an archive — use Extract.")
                    : FileVerbState.On(header);
            }

            case FileVerb.Paste:
                if (inArchive) return FileVerbState.Off("Paste", InsideArchive);
                return s.ClipboardHasFiles?.Invoke() == true
                    ? FileVerbState.On("Paste")
                    : FileVerbState.Off("Paste", "There are no files on the clipboard.");

            case FileVerb.CopyPath:
                return Selected(s, s.Count > 1 ? "Copy as paths" : "Copy as path");

            case FileVerb.CopyName:
                return Selected(s, s.Count > 1 ? "Copy names" : "Copy name");

            // Real files only: a checksum answers for a file's bytes, a folder has none of its own,
            // and an entry inside a container has no path the digester can open. Not offered on an
            // empty selection either — digesting a whole folder is a different, much slower verb.
            case FileVerb.Checksum:
            {
                var header = s.Files > 1 ? $"Checksums of {s.Files:N0} files…" : "Checksum…";
                if (inArchive) return FileVerbState.Off(header, InsideArchive);
                if (s.Count == 0) return FileVerbState.Off(header, NothingSelected);
                return s.Files == s.Count
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "A checksum is of a file; the selection includes a folder.");
            }

            // Verifying is a verb about a folder, so it follows the folder verbs' rule rather than
            // the selection's — a checksum file describes what is around it.
            case FileVerb.VerifyChecksums:
                return inArchive
                    ? FileVerbState.Off("Verify checksums…", InsideArchive)
                    : FileVerbState.On("Verify checksums…");

            // Exactly two real files. Deliberately not conditional on a folder comparison being
            // under way: "are these two the same?" is the general question, and the comparison
            // session is only one of the ways of arriving at it.
            case FileVerb.CompareFiles:
            {
                const string header = "Compare these two files…";
                if (inArchive) return FileVerbState.Off(header, InsideArchive);
                return s.Count == 2 && s.Files == 2
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "Select exactly two files.");
            }

            // Only while a folder comparison is up: this asks it to re-judge these rows by their
            // bytes, and there is nothing to re-judge otherwise. Hidden rather than greyed, because
            // a disabled item nobody can explain is worse than an absent one.
            case FileVerb.SettleByContent:
            {
                var header = s.Files > 1 ? "Settle these by content" : "Settle by content";
                if (!s.Comparing)
                    return new FileVerbState(false, false, header, "Only while two panes are being compared.");
                if (inArchive) return FileVerbState.Off(header, InsideArchive);
                return s.Files > 0
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "Select the files to settle.");
            }

            case FileVerb.Open:
                return Selected(s, "Open");

            // "Open in new tab/pane" only makes sense for folders.
            case FileVerb.OpenInNewTab:
            {
                var header = s.Folders > 1 ? $"Open {s.Folders} folders in new tabs" : "Open in new tab";
                return s.Folders > 0 ? FileVerbState.On(header) : FileVerbState.Off(header, "Select a folder.");
            }

            case FileVerb.OpenInNewPane:
                return s.Folders > 0
                    ? FileVerbState.On("Open in new pane")
                    : FileVerbState.Off("Open in new pane", "Select a folder.");

            // Both act on the selection or, with nothing selected, on the folder being shown — so
            // an empty-space right-click opens this folder rather than doing nothing. Off inside a
            // container: an entry's path is virtual and names nothing another program can open.
            case FileVerb.OpenInTerminal:
            case FileVerb.OpenInVSCode:
            {
                var header = verb == FileVerb.OpenInTerminal ? "Open in Terminal" : "Open in VS Code";
                if (inArchive) return FileVerbState.Off(header, InsideArchive);
                return s.Count > 0 || s.HasFolder
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "There is no folder here to open.");
            }

            // One folder, since the view analyses a single root. With nothing selected this still
            // offers itself and analyses the folder being shown, which is the useful reading of an
            // empty-space right-click.
            case FileVerb.DiskUsage:
                return OneFolderOrNone(s, "Analyse disk usage…");

            // Duplicates reads whole files by path, which an entry does not have. Disk usage does
            // not: every size inside a container is already exact, so it stays on.
            case FileVerb.Duplicates:
                return inArchive
                    ? FileVerbState.Off("Find duplicates…", InsideArchive)
                    : OneFolderOrNone(s, "Find duplicates…");

            // The change log is keyed by real paths, and an entry inside a container has none.
            case FileVerb.Changes:
                return inArchive
                    ? FileVerbState.Off("What changed here…", InsideArchive)
                    : OneFolderOrNone(s, "What changed here…");

            // Not keyed on the selection at all, and never disabled: it compares the two panes, so
            // what it needs is that there be two, and pressing it is how you find that out — a
            // greyed row with no explanation is exactly the thing this feature answers with a modal
            // instead. The header flips because pressing it again is how a comparison is stopped,
            // and a menu that still said "Compare" would be lying about what it does.
            case FileVerb.ComparePanes:
                return FileVerbState.On(s.Comparing ? "Stop comparing" : "Compare with other pane");

            // Extract shows from either side of the container: on a single selected archive out
            // here, or on the selection (or everything, with nothing selected) in there. It is the
            // one write verb that is *more* available inside an archive than outside one.
            case FileVerb.ExtractHere:
            case FileVerb.ExtractTo:
            {
                var header = verb == FileVerb.ExtractTo
                    ? "Extract to…"
                    : inArchive && s.Count > 0 ? $"Extract {s.Count:N0} item(s) here" : "Extract here";
                return inArchive || s.SingleIsArchive
                    ? FileVerbState.On(header)
                    : new FileVerbState(false, false, header, "Select one archive, or open one.");
            }

            // Compressing reads files by path, so it needs real ones — off inside a container and
            // off over a search result, where "the folder being shown" is not a folder. A flat
            // branch view is flattened too but does have one, so it keeps this: IsSearchResult, not
            // IsFlattened.
            case FileVerb.Compress:
            {
                var header = s.Count > 1 ? $"Compress {s.Count:N0} items…" : "Compress…";
                if (inArchive) return FileVerbState.Off(header, InsideArchive);
                if (s.IsSearchResult) return FileVerbState.Off(header, "Not available over search results.");
                return s.HasFolder
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "There is no folder here to compress.");
            }

            // Only ever one file: "run this folder as administrator" means nothing, and a whole
            // selection of programs started elevated at once is not something to offer from a menu.
            // Only where there is something to elevate. A runas verb is registered per file type —
            // exefile has one, txtfile does not and never will — so offering it on a type without
            // one produces ERROR_NO_ASSOCIATION and nothing else. Where there is no verb but the
            // file has a handler, that handler is what gets elevated (a .sln opens VSLauncher as
            // administrator); greyed out means neither was available. See RunAsVerbRules.Decide.
            case FileVerb.RunAsAdmin:
            {
                const string header = "Run as administrator";
                if (s.Count != 1) return FileVerbState.Off(header, "Select one item.");
                return s.CanRunElevated?.Invoke() == true
                    ? FileVerbState.On(header)
                    : FileVerbState.Off(header, "Nothing about this item can be run as administrator.");
            }

            // Rename and Delete stay on inside a container, but they mean something different in
            // there: the container is rewritten beside itself and swapped in. Whether that is
            // possible at all depends on the format and the archive's own shape, and the planner is
            // what knows — so the menu offers it and the refusal, when there is one, arrives by
            // name.
            //
            // Renaming several at once is not offered in there: a rewrite writes each entry exactly
            // once, so the staging trick that makes a rotating batch work on disk has nowhere to
            // happen.
            case FileVerb.Rename:
            {
                var header = s.Count > 1 ? $"Rename {s.Count} items…" : "Rename…";
                if (s.Count == 0) return FileVerbState.Off(header, NothingSelected);
                return s.Count > 1 && inArchive
                    ? FileVerbState.Off(header, "Inside an archive, rename one item at a time.")
                    : FileVerbState.On(header);
            }

            case FileVerb.Delete:
                return Selected(s, s.Count > 1 ? $"Delete {s.Count} items…" : "Delete…");

            // Shift+Delete has no meaning in there: an entry has no Recycle Bin and no staging of
            // its own, so there is no second, more destructive thing for it to mean.
            case FileVerb.DeletePermanently:
            {
                const string header = "Delete permanently…";
                if (s.Count == 0) return FileVerbState.Off(header, NothingSelected);
                return inArchive
                    ? FileVerbState.Off(header, "Inside an archive there is only Delete.")
                    : FileVerbState.On(header);
            }

            // Bookmarking is refused at BookmarkService too, and that is the important half: a
            // virtual path in the bookmark table would sort strictly inside the archive's own
            // containing folder under PathKey.IsUnder, and poison every subtree query over it.
            // "Remove bookmark" only when every selected item is already bookmarked.
            case FileVerb.Bookmark:
            {
                var header = s.AllBookmarked ? "Remove bookmark" : "Bookmark";
                if (s.Count == 0) return FileVerbState.Off(header, NothingSelected);
                return inArchive ? FileVerbState.Off(header, InsideArchive) : FileVerbState.On(header);
            }

            case FileVerb.Properties:
                return Selected(s, "Properties…");

            // New acts on the folder being shown, so it needs one — and a search result is not one:
            // creating into the search root would produce an item that may not match the query and
            // so would not appear, which reads as a failure. That objection is exactly what does
            // not apply to a flat branch view, where a new child of the root is in the listing by
            // definition.
            case FileVerb.New:
                if (inArchive) return FileVerbState.Off("New", InsideArchive);
                if (s.IsSearchResult) return FileVerbState.Off("New", "Not available over search results.");
                return s.HasFolder
                    ? FileVerbState.On("New")
                    : FileVerbState.Off("New", "There is no folder here to create in.");

            case FileVerb.UnlockArchive:
                return s.ArchiveLocked
                    ? FileVerbState.On("Unlock…")
                    : FileVerbState.Off("Unlock…", "This tab is not showing a locked archive.");

            default:
                throw new ArgumentOutOfRangeException(nameof(verb), verb, null);
        }
    }

    private static FileVerbState Selected(FileVerbSnapshot s, string header) =>
        s.Count > 0 ? FileVerbState.On(header) : FileVerbState.Off(header, NothingSelected);

    private static FileVerbState OneFolderOrNone(FileVerbSnapshot s, string header) =>
        s.Count == 0 || (s.Count == 1 && s.Folders == 1)
            ? FileVerbState.On(header)
            : FileVerbState.Off(header, "Select one folder, or nothing for the folder being shown.");
}
