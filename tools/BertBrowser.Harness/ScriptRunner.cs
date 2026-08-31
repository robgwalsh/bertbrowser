using System.Globalization;
using System.IO.Compression;
using System.Text;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Data;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Columns;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.Elevation;
using BertBrowser.Core.Services.DiskUsage;
using BertBrowser.Core.Services.Duplicates;
using BertBrowser.Core.Services.Mft;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Preview;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.Harness;

/// <summary>Raised when a command asserts something that is not so.</summary>
internal sealed class AssertionException(string message) : Exception(message);

/// <summary>
/// Runs a script against a hosted browser window.
/// </summary>
/// <remarks>
/// <para>
/// The command set is deliberately small and its vocabulary is the app's own: paths, row names,
/// panes and tabs, and the names of elements as the XAML spells them. Every command goes through
/// the view model the interface goes through, so a script cannot reach past what a user can do.
/// </para>
/// <para>
/// Two things are refused rather than driven. Anything that starts another program — the
/// <see cref="RefusingProcessLauncher"/> is what the window is given — because that program's
/// window would land on the user's desktop. And the clipboard, because there is one per session
/// and a scripted copy would throw away whatever the user had in theirs; <c>move</c> and
/// <c>copy</c> go through the same transfer engine paste does, without touching it.
/// </para>
/// </remarks>
internal sealed class ScriptRunner(UiSession session, HarnessOptions options, TextWriter output)
{
    private readonly Sandbox _sandbox = new(options);
    private readonly DeniedFixture _denied = new();
    private int _shots;

    /// <summary>Runs every command. Returns the process exit code.</summary>
    public int Run()
    {
        var failed = false;

        foreach (var raw in options.Commands)
        {
            var line = Strip(raw);
            if (line.Length == 0) continue;

            try
            {
                Execute(line);
                output.WriteLine($"OK {line}");
            }
            catch (Exception e) when (e is AssertionException or FormatException or InvalidOperationException
                                       or TimeoutException or IOException or ArgumentException
                                       or UnauthorizedAccessException)
            {
                output.WriteLine($"FAIL {line} — {e.Message}");
                failed = true;

                if (!options.KeepGoing) break;
            }
        }

        // Always, and before the scratch directory is swept: a Deny ACE left in place makes the
        // sandbox undeletable, and every later run would leave another one behind in %TEMP%.
        _denied.Release();

        return failed ? HarnessOptions.Exit.Failed : HarnessOptions.Exit.Ok;
    }

    /// <summary>Drops comments and surrounding space; '#' only starts one at the front of a line.</summary>
    private static string Strip(string line)
    {
        var trimmed = line.Trim();
        return trimmed.StartsWith('#') ? "" : trimmed;
    }

    private void Execute(string line)
    {
        var (verb, rest) = Split(line);

        switch (verb.ToLowerInvariant())
        {
            case "echo": output.WriteLine(rest); break;
            case "sleep": Thread.Sleep(Number(rest, "sleep")); break;
            case "settle": session.Settle(rest.Length == 0 ? 0 : Number(rest, "settle")); break;

            // fixtures
            case "tree": Tree(rest); break;
            case "mkdir": MakeDirectory(rest); break;
            case "write": WriteFile(rest); break;
            case "sandbox": output.WriteLine(_sandbox.Root); break;
            case "deny": Deny(rest); break;

            // navigation
            case "go": Go(rest); break;
            case "up": Invoke(() => session.Tab.UpCommand.Execute(null)); break;
            case "back": Invoke(() => session.Tab.BackCommand.Execute(null)); break;
            case "forward": Invoke(() => session.Tab.ForwardCommand.Execute(null)); break;
            case "refresh": Invoke(() => session.Tab.RefreshCommand.Execute(null)); break;
            case "enter": Enter(rest); break;
            case "tree-click": TreeClick(rest); break;

            // selection
            case "select": Select(rest); break;
            case "select-all": SelectAll(); break;
            case "deselect": SetSelection([]); break;

            // tabs and panes
            case "newtab": NewTab(rest); break;
            case "closetab": Invoke(() => session.Shell.ActivePane.CloseTab(session.Tab)); break;
            case "reopen": Invoke(() => session.Shell.ActivePane.ReopenClosedTab()); break;
            case "tab": ActivateTab(rest); break;
            case "split": Split(rest, out _); break;
            case "closepane": Invoke(() => session.Shell.ClosePane(session.Shell.ActivePane)); break;
            case "pane": ActivatePane(rest); break;

            // searching
            case "search": Search(rest, global: false); break;
            case "gsearch": Search(rest, global: true); break;
            case "clear-search": Invoke(() => session.Tab.ClearSearchCommand.Execute(null)); break;

            // acting on the selection
            case "newfolder": NewItem(rest, NewItemKind.Folder); break;
            case "newfile": NewItem(rest, NewItemKind.File); break;
            case "rename": Rename(rest); break;
            case "rename-rule": RenameByRule(rest); break;
            case "delete": Delete(rest, DeleteMode.Recycle); break;
            case "delete-permanent": Delete(rest, DeleteMode.Permanent); break;
            case "move": Transfer(rest, TransferVerb.Move); break;
            case "copy": Transfer(rest, TransferVerb.Copy); break;
            case "undo": Undo(); break;
            case "progress-demo": ProgressDemo(rest); break;
            case "archive-fixture": ArchiveFixture(rest); break;
            case "extract": Extract(rest); break;
            case "compress": Compress(rest); break;
            case "unlock": Unlock(rest); break;
            case "duplicates": Duplicates(rest); break;
            case "duplicates-keep": DuplicatesKeep(rest); break;
            case "duplicates-remove": DuplicatesRemove(); break;

            // browse settings
            case "hidden": Hidden(rest); break;
            case "thumbnails": Thumbnails(rest); break;
            case "preview": Preview(rest); break;
            case "preview-mode": PreviewViewMode(rest); break;
            case "preview-fixture": PreviewFixture(rest); break;
            case "content-fixture": ContentFixture(rest); break;
            case "sort": Sort(rest); break;
            case "theme": Theme(rest); break;

            // capturing and reading back
            case "shot": Shot(rest); break;
            case "dialog": Dialog(rest); break;
            case "state": output.WriteLine("STATE " + State()); break;
            case "session": Session(); break;
            case "probe": Probe(rest); break;
            case "rows": output.WriteLine("ROWS " + string.Join(", ", RowNames())); break;
            case "columns": Columns(rest); break;
            case "assert-column": AssertColumn(rest, expected: true); break;
            case "assert-no-column": AssertColumn(rest, expected: false); break;
            case "assert-columns": AssertColumns(rest); break;
            case "assert-metadata": AssertMetadata(rest); break;
            case "menu": Menu(rest); break;
            case "assert-header-menu": AssertHeaderMenu(rest); break;

            // assertions
            case "assert-path": AssertPath(rest); break;
            case "assert-status": AssertStatus(rest); break;
            case "assert-error": AssertError(rest); break;
            case "assert-match": AssertMatch(rest); break;
            case "stop-search": Invoke(() => session.Tab.StopSearchCommand.Execute(null)); break;
            case "assert-indexing": AssertIndexing(rest); break;
            case "assert-transfer": AssertTransfer(rest); break;
            case "assert-transfer-indeterminate": AssertTransferIndeterminate(); break;
            case "assert-count": AssertCount(rest); break;
            case "assert-row": AssertRow(rest, expected: true); break;
            case "assert-no-row": AssertRow(rest, expected: false); break;
            case "assert-selected": AssertSelected(rest); break;
            case "assert-preview": AssertPreview(rest); break;
            case "assert-tabs": AssertTabs(rest); break;
            case "assert-panes": AssertPanes(rest); break;
            case "assert-flattened": AssertFlattened(expected: true); break;
            case "assert-not-flattened": AssertFlattened(expected: false); break;
            case "assert-inside-archive": AssertInsideArchive(expected: true); break;
            case "assert-not-inside-archive": AssertInsideArchive(expected: false); break;
            case "assert-can-undo": AssertCanUndo(expected: true); break;
            case "assert-cannot-undo": AssertCanUndo(expected: false); break;
            case "assert-duplicate-groups": AssertDuplicateGroups(rest); break;
            case "assert-duplicate-selected": AssertDuplicateSelected(rest); break;
            case "assert-duplicate-row": AssertDuplicateRow(rest, expected: true); break;
            case "assert-no-duplicate-row": AssertDuplicateRow(rest, expected: false); break;
            case "assert-exists": AssertOnDisk(rest, expected: true); break;
            case "assert-missing": AssertOnDisk(rest, expected: false); break;
            case "assert-visible": AssertVisibility(rest, expected: true); break;
            case "assert-hidden": AssertVisibility(rest, expected: false); break;
            case "assert-not-launched": AssertNotLaunched(); break;
            case "assert-elevation-offered": AssertElevationOffered(expected: true); break;
            case "assert-no-elevation-offered": AssertElevationOffered(expected: false); break;

            default:
                throw new FormatException($"'{verb}' is not a command. Run with --help for the list.");
        }
    }

    // ---- fixtures ---------------------------------------------------------------------

    private void Tree(string rest)
    {
        var root = _sandbox.Populate(rest.Length == 0 ? "." : rest);
        output.WriteLine($"# tree: {root}");
    }

    /// <summary>
    /// Lays down files the preview pane can actually show, in a <c>Preview</c> folder of their own.
    /// </summary>
    /// <remarks>
    /// Separate from <see cref="Tree"/> and not folded into it, for two reasons. The ordinary
    /// fixture's files are text with the right extensions — <c>photo.jpg</c> is not a JPEG — which
    /// is fine for a listing and useless for a preview; and adding files to that fixture would move
    /// every <c>assert-count</c> in every existing script.
    ///
    /// What lands here is generated rather than shipped as binary test data, so it is deterministic
    /// and reviewable: the PNG is drawn from a formula and carries transparency, so it also proves
    /// the chequerboard behind it.
    /// </remarks>
    private void PreviewFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "Preview" : rest, "preview-fixture");
        Directory.CreateDirectory(root);

        WriteSamplePng(Path.Combine(root, "sample.png"));
        WriteSampleZip(Path.Combine(root, "sample.zip"));
        File.WriteAllText(Path.Combine(root, "Sample.cs"), SampleCode);
        File.WriteAllText(Path.Combine(root, "sample.md"), SampleMarkdown);
        Sandbox.Write(Path.Combine(root, "plain.txt"), 400);

        // The two shapes of "the shell has nothing and the extension table has no entry either".
        // A .manifest is now in the table; a .qqq never will be, and is here because the content
        // sniff is what has to carry it.
        File.WriteAllText(Path.Combine(root, "app.exe.manifest"), SampleManifest, new UTF8Encoding(true));
        File.WriteAllText(Path.Combine(root, "mystery.qqq"), "Not a format anyone has heard of,\nand plainly readable anyway.\n");

        // And the other half of the rule: something that really is binary must still be refused
        // rather than shown as a screen of mojibake.
        File.WriteAllBytes(Path.Combine(root, "opaque.qqq"), OpaqueBytes());

        Sandbox.Stamp(root);
        output.WriteLine($"# preview fixture: {root}");
    }

    /// <summary>
    /// Files whose <em>contents</em> are the assertion, for driving <c>content:</c>.
    /// </summary>
    /// <remarks>
    /// <para>Its own fixture rather than an addition to <c>tree</c> or <c>preview-fixture</c>, for
    /// the reason spelled out on the latter: adding files to a shared fixture moves every
    /// <c>assert-count</c> in every script that uses it. And <c>Sandbox.Write</c> cannot serve
    /// here, because it fills a file with its own name repeated — which is exactly the case a
    /// content search has to be proved <em>not</em> to be answering.</para>
    /// <para>Each file is here for one rule. <c>needle.txt</c> is named for the needle and does not
    /// contain it, and <c>haystack.txt</c> is the other way round: together they are what proves
    /// the reader ran at all rather than the name term quietly answering. <c>decoy.txt</c> holds
    /// both words of the phrase but never adjacently. <c>utf16.txt</c> exercises the
    /// bomless-UTF-16 rung of the encoding ladder end to end. <c>opaque.bin</c> spells the needle
    /// in plain bytes behind a NUL, so a hit on it would mean the binary sniff had stopped working.
    /// </para>
    /// </remarks>
    private void ContentFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "Content" : rest, "content-fixture");
        Directory.CreateDirectory(root);

        File.WriteAllText(Path.Combine(root, "haystack.txt"),
            "first line\nthe annual report is late\nthird line\n");
        File.WriteAllText(Path.Combine(root, "decoy.txt"),
            "annual sales figures\nand a separate report\n");
        File.WriteAllText(Path.Combine(root, "code.cs"),
            "public class Thing\n{\n    // the annual report goes here\n}\n");

        // Named for the needle, and does not contain it.
        File.WriteAllText(Path.Combine(root, "annual report.md"),
            "this file is named for it but never says the words\n");

        // UTF-16 LE with no byte-order mark — what most Windows tools emit.
        File.WriteAllText(Path.Combine(root, "utf16.txt"),
            "the annual report, in two-byte characters\n", new UnicodeEncoding(false, false));

        var opaque = new byte[2048];
        opaque[0] = 0;
        Encoding.ASCII.GetBytes("the annual report").CopyTo(opaque, 16);
        for (var i = 64; i < opaque.Length; i++) opaque[i] = (byte)(i % 251);
        File.WriteAllBytes(Path.Combine(root, "opaque.bin"), opaque);

        Sandbox.Stamp(root);
        output.WriteLine($"# content fixture: {root}");
    }

    /// <summary>A 64×64 PNG with an alpha hole through the middle, drawn from a formula so two
    /// runs produce byte-identical files.</summary>
    private static void WriteSamplePng(string path)
    {
        const int size = 64;
        var pixels = new byte[size * size * 4];
        for (var y = 0; y < size; y++)
        {
            for (var x = 0; x < size; x++)
            {
                var i = (y * size + x) * 4;
                var inside = (x - 32) * (x - 32) + (y - 32) * (y - 32) < 20 * 20;
                var alpha = (byte)(inside ? 0 : 255);
                // Premultiplied would need the colour scaled by alpha; Bgra32 does not.
                pixels[i + 0] = (byte)(x * 4);       // B
                pixels[i + 1] = (byte)(y * 4);       // G
                pixels[i + 2] = 200;                 // R
                pixels[i + 3] = alpha;
            }
        }

        var bitmap = BitmapSource.Create(size, size, 96, 96, PixelFormats.Bgra32, null, pixels, size * 4);
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(bitmap));

        using var file = File.Create(path);
        encoder.Save(file);
    }

    private static void WriteSampleZip(string path)
    {
        using var file = File.Create(path);
        using var archive = new ZipArchive(file, ZipArchiveMode.Create);

        foreach (var (name, text) in new[]
        {
            ("readme.txt", "This archive is listed, never extracted.\n"),
            ("src/app.js", "console.log('hello');\n"),
            ("src/lib/util.js", "export const one = 1;\n"),
            ("docs/guide.md", "# Guide\n\nNothing here is opened.\n"),
        })
        {
            using var entry = archive.CreateEntry(name).Open();
            entry.Write(System.Text.Encoding.UTF8.GetBytes(text));
        }
    }

    private const string SampleCode = """
        using System;

        namespace Sample;

        /// <summary>A little of everything the tokenizer has an opinion about.</summary>
        public static class Greeter
        {
            private const int Attempts = 3;      // a number and a line comment
            private const string Url = "https://example.com/not-a-comment";

            /* a block comment
               over two lines */
            public static void Greet(string name)
            {
                for (var i = 0; i < Attempts; i++)
                    Console.WriteLine($"Hello, {name}! 0xFF is {0xFF}, and it's fine.");
            }
        }
        """;

    /// <summary>A side-application manifest, shaped like the ones that sit beside an .exe — the
    /// case that started this: plainly XML, no shell handler, and previously refused outright.</summary>
    private const string SampleManifest = """
        <?xml version="1.0" encoding="utf-8"?>
        <assembly manifestVersion="1.0" xmlns="urn:schemas-microsoft-com:asm.v1">
          <assemblyIdentity version="1.0.0.0" name="sample.app"/>
          <trustInfo>
            <security>
              <requestedPrivileges>
                <!-- asInvoker, like the app itself -->
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
              </requestedPrivileges>
            </security>
          </trustInfo>
        </assembly>
        """;

    /// <summary>Bytes with no NUL in them, so the loose binary check passes — but a wall of control
    /// characters once decoded, which is what the strict check is for.</summary>
    private static byte[] OpaqueBytes()
    {
        var bytes = new byte[2048];
        for (var i = 0; i < bytes.Length; i++) bytes[i] = (byte)(1 + i % 0x1F);
        return bytes;
    }

    private const string SampleMarkdown = """
        # Sample document

        Ordinary paragraph text, which stays plain.

        - a bullet
        - another bullet

        > a quoted line

        ```csharp
        var inside = "a fenced block";
        ```

        The end.
        """;

    /// <summary>Makes a file the current account may not delete or move — a real access-denied
    /// failure, produced with no privilege and no prompt. See DeniedFixture for why the ACE goes on
    /// the folder before the file exists.</summary>
    private void Deny(string rest) =>
        _denied.Deny(_sandbox.RequireInside(Require(rest, "deny"), "deny"));

    private void MakeDirectory(string rest) =>
        Directory.CreateDirectory(_sandbox.RequireInside(Require(rest, "mkdir"), "mkdir"));

    private void WriteFile(string rest)
    {
        var (path, tail) = Split(rest);
        var bytes = tail.Length == 0 ? 64 : Number(tail, "write");

        Sandbox.Write(_sandbox.RequireInside(Require(path, "write"), "write"), bytes);
    }

    // ---- navigation -------------------------------------------------------------------

    private void Go(string rest)
    {
        var path = _sandbox.Resolve(Require(rest, "go"));
        // Somewhere inside an archive is a legitimate destination and is not a directory on disk,
        // so the check asks the same question navigation does rather than only Directory.Exists.
        if (!Directory.Exists(path) &&
            BertBrowser.Core.Services.Archives.ArchivePath.Parse(path, File.Exists) is null)
            throw new AssertionException($"There is no folder at '{path}'.");

        Await(() => session.Tab.NavigateToAsync(path));
    }

    /// <summary>Opens the named row, exactly as double-clicking or pressing Enter on it does.</summary>
    /// <remarks>
    /// A file is refused rather than opened: the launcher this run is given starts nothing, so the
    /// only outcome would be a status-bar message, and saying so up front beats a script that looks
    /// like it did something.
    /// <para>
    /// An archive is the exception, and it is not a special case in the harness so much as the
    /// absence of one: <c>Tab.Open</c> navigates into it rather than launching it, so nothing
    /// starts and the refusal has nothing to protect. Letting the script say <c>enter sample.zip</c>
    /// is also what makes the capture exercise the real double-click path rather than <c>go</c>.
    /// </para>
    /// </remarks>
    private void Enter(string rest)
    {
        var name = Require(rest, "enter");
        var row = Row(name);

        if (!row.IsDirectory && BertBrowser.Core.Services.Archives.ArchiveFormats.IsArchiveName(name))
        {
            Invoke(() => session.Tab.Open(row, elevated: false));
            session.Settle();
            return;
        }

        if (!row.IsDirectory)
            throw new InvalidOperationException(
                $"'{name}' is a file, and this run starts no programs. Use 'go' for folders, or " +
                "assert on the status bar if you meant to check the refusal.");

        Invoke(() => session.Tab.Open(row, elevated: false));
        session.Settle();
    }

    /// <summary>
    /// Clicks a folder in the sidebar tree.
    /// </summary>
    /// <remarks>
    /// Setting <c>IsSelected</c> on the node is exactly what a click does — the <c>TreeViewItem</c>
    /// writes it back through the container style's two-way binding, and the tree turns that into
    /// a navigation. Which makes this the one way to check that the guards keeping the tree from
    /// announcing its <em>own</em> selections have not also swallowed a real one. The folder has to
    /// be showing already; navigate to it or its parent first.
    /// </remarks>
    private void TreeClick(string rest)
    {
        var path = _sandbox.Resolve(Require(rest, "tree-click"));

        session.Dispatcher.Invoke(() =>
        {
            var node = TreeNode(session.Shell.Tree.Roots.OfType<DirectoryNodeViewModel>(), path)
                ?? throw new AssertionException(
                    $"The tree has no row for '{path}' showing; navigate there or to its parent first.");

            node.IsSelected = true;
        });

        // The tree's own reveal is debounced, and the navigation it kicks off is a listing load.
        session.Settle(quietMs: 200);
    }

    private static DirectoryNodeViewModel? TreeNode(
        IEnumerable<DirectoryNodeViewModel> nodes, string path)
    {
        foreach (var node in nodes)
        {
            if (node.FullPath.Length == 0) continue; // unexpanded-node placeholder
            if (node.FullPath.Equals(path, StringComparison.OrdinalIgnoreCase)) return node;
            if (TreeNode(node.Children, path) is { } found) return found;
        }

        return null;
    }

    // ---- selection --------------------------------------------------------------------

    /// <summary>
    /// Selects rows by name.
    /// </summary>
    /// <remarks>
    /// Through the <c>ListView</c> rather than the view model's <c>SelectedItems</c>, because that
    /// property is a mirror the view writes: setting it would leave the rows unhighlighted in every
    /// capture and the status bar's selection summary describing the previous selection.
    /// </remarks>
    private void Select(string rest)
    {
        var names = Require(rest, "select").Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        SetSelection(names.Select(Row).ToList());
    }

    private void SelectAll() => Invoke(() =>
        SetSelection(session.Tab.FileList.Items.ToList()));

    private void SetSelection(IReadOnlyList<FileItemViewModel> items) => Invoke(() =>
    {
        var list = FileList();
        list.SelectedItems.Clear();
        foreach (var item in items) list.SelectedItems.Add(item);

        session.Settle();
    });

    private ListView FileList() =>
        FindNamed<ListView>("FileListView")
        ?? throw new InvalidOperationException("The active tab has no file list.");

    // --- Columns ---
    //
    // Read off the live GridView rather than out of the view model, deliberately: the view model's
    // answer is what the rules decided, and what a script needs to know is what the reconcile
    // actually built. assert-visible cannot help here — a GridViewColumn is not a FrameworkElement
    // and never appears in the visual tree.

    private IReadOnlyList<GridViewColumn> LiveColumns() => session.Dispatcher.Invoke(() =>
        FileList().View is GridView grid
            ? grid.Columns.ToList()
            : throw new AssertionException(
                "The file list is in tile mode, which has no columns."));

    private string ColumnSummary() => session.Dispatcher.Invoke(() => string.Join(", ",
        LiveColumns().Select(c =>
            $"{FileListColumns.GetId(c)}:{c.ActualWidth:0}")));

    private void Columns(string rest)
    {
        var (verb, tail) = Split(rest);
        switch (verb.ToLowerInvariant())
        {
            case "":
                output.WriteLine("COLUMNS " + ColumnSummary());
                break;

            case "add":
                Edit(l => ColumnLayoutRules.Toggle(l, Require(tail, "columns add"), on: true));
                break;

            case "remove":
                Edit(l => ColumnLayoutRules.Toggle(l, Require(tail, "columns remove"), on: false));
                break;

            case "move":
            {
                var (id, index) = Split(tail);
                Edit(l => ColumnLayoutRules.Move(l, id, Number(index, "columns move")));
                break;
            }

            case "width":
            {
                var (id, width) = Split(tail);
                Edit(l => ColumnLayoutRules.SetWidth(l, id, Number(width, "columns width")));
                break;
            }

            case "reset":
                Invoke(() => session.Tab.FileList.ColumnLayout = null);
                break;

            default:
                throw new FormatException(
                    $"columns wants add, remove, move, width or reset, got '{verb}'.");
        }

        void Edit(Func<IReadOnlyList<ColumnSetting>?, IReadOnlyList<ColumnSetting>> change) =>
            Invoke(() =>
            {
                var list = session.Tab.FileList;
                list.ColumnLayout = change(list.ColumnLayout);
            });
    }

    private void AssertColumn(string rest, bool expected)
    {
        var id = Require(rest, expected ? "assert-column" : "assert-no-column");
        var present = session.Dispatcher.Invoke(() =>
            LiveColumns().Any(c =>
                FileListColumns.GetId(c).Equals(id, StringComparison.OrdinalIgnoreCase)));

        if (present != expected)
            throw new AssertionException(expected
                ? $"there is no '{id}' column: {ColumnSummary()}"
                : $"the '{id}' column is still there: {ColumnSummary()}");
    }

    /// <summary>
    /// Which menu a right-click on the empty strip past the last column would open.
    /// </summary>
    /// <remarks>
    /// Resolved the way <c>ContextMenuService</c> resolves it — the nearest ancestor carrying a menu
    /// — rather than by asserting that some particular element holds one, so it stays true however
    /// the header strip is put together. That gap is a <c>Role=Padding</c> header the presenter
    /// builds itself, so it has no menu of its own and used to fall through to the file list's.
    /// </remarks>
    private void AssertHeaderMenu(string rest)
    {
        var want = Require(rest, "assert-header-menu").ToLowerInvariant();
        if (want is not ("columns" or "files"))
            throw new FormatException($"assert-header-menu wants columns or files, got '{want}'.");

        var actual = session.Dispatcher.Invoke(() =>
        {
            var presenter = VisualTreeUtil.FindDescendant<GridViewHeaderRowPresenter>(FileList())
                ?? throw new AssertionException("the file list has no header row.");

            var padding = Descendants(presenter)
                .OfType<GridViewColumnHeader>()
                .FirstOrDefault(h => h.Role == GridViewColumnHeaderRole.Padding)
                ?? throw new AssertionException("the header row has no padding filler to click on.");

            // Identified by reference, not by what is in it: the column menu fills itself in on
            // Opened, so its Items are empty until someone right-clicks and looking for a known
            // entry would call it the file menu every time.
            var columnMenu = VisualTreeUtil.FindAncestor<DirectoryTabView>(padding)?.ColumnMenuForHarness;

            for (DependencyObject? d = padding; d is not null; d = VisualTreeHelper.GetParent(d))
            {
                if (d is FrameworkElement { ContextMenu: { } menu })
                    return ReferenceEquals(menu, columnMenu) ? "columns" : "files";
            }
            return "none";
        });

        if (actual != want)
            throw new AssertionException(
                $"a right-click past the last column would open the '{actual}' menu, expected '{want}'.");
    }

    /// <summary>
    /// Photographs a context menu's items.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The items are hosted in the parked window and rendered there, rather than the menu being
    /// opened for real. <b>A ContextMenu is a Popup and gets its own top-level window</b>, which WPF
    /// repositions to fit the nearest monitor — so opening one would put a menu on the user's
    /// screen, which is the single thing this harness exists to avoid.
    /// </para>
    /// <para>
    /// What that still covers is what matters here: these are the real
    /// <see cref="ColumnHeaderMenu"/>'s own <c>MenuItem</c> instances under the app's own implicit
    /// <c>MenuItem</c> style, so a check mark that does not render is visible in the picture. It
    /// does not cover the popup's own chrome, and does not claim to.
    /// </para>
    /// </remarks>
    private void Menu(string rest)
    {
        var (kind, tail) = Split(rest);
        var (name, _) = Split(tail);

        var items = session.Dispatcher.Invoke(() => kind.ToLowerInvariant() switch
        {
            "columns" => ColumnMenuItems(),
            var other => throw new FormatException($"'{other}' is not a menu. Try: columns."),
        });

        var path = Resolve(Named(name.Length == 0 ? $"menu-{kind}" : name, ++_shots));
        session.Dispatcher.Invoke(() => RenderDetached(items, path));

        if (!Capture.HasContent(path))
            throw new AssertionException($"{path} is a single flat colour — the menu rendered nothing.");
        output.WriteLine($"SHOT {path}");
    }

    /// <summary>
    /// Renders loose menu items straight to a PNG.
    /// </summary>
    /// <remarks>
    /// Measured and arranged with no parent, which is what makes this correct: <c>Render</c> applies
    /// a visual's offset within its parent, so the usual trap — a child drawn at its window
    /// coordinates and mostly missing the bitmap — cannot arise when there is no parent to be offset
    /// within. It also needs no <c>VisualBrush</c>, whose cached realisation is the reason
    /// <see cref="Capture"/> renders the whole window and crops instead.
    /// </remarks>
    private static void RenderDetached(IReadOnlyList<MenuItem> items, string path)
    {
        var host = new StackPanel { Width = 300 };
        host.SetResourceReference(Panel.BackgroundProperty, BertBrowser.Core.Theming.ThemeToken.MenuBackground);
        foreach (var item in items) host.Children.Add(item);

        host.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        host.Arrange(new Rect(host.DesiredSize));
        host.UpdateLayout();

        var width = (int)Math.Ceiling(host.ActualWidth);
        var height = (int)Math.Ceiling(host.ActualHeight);
        if (width <= 0 || height <= 0)
            throw new AssertionException($"the menu laid out to {width}x{height}, so there is nothing to capture.");

        var target = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        target.Render(host);

        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(target));
        if (Path.GetDirectoryName(path) is { Length: > 0 } directory) Directory.CreateDirectory(directory);
        using var stream = File.Create(path);
        encoder.Save(stream);
    }

    /// <summary>The real column menu's items, built the way a right-click builds them.</summary>
    private List<MenuItem> ColumnMenuItems()
    {
        var view = FindNamed<FrameworkElement>("FileListView");
        var tabView = VisualTreeUtil.FindAncestor<DirectoryTabView>(view)
            ?? throw new AssertionException("The file list is not inside a DirectoryTabView.");

        var menu = tabView.ColumnMenuForHarness;
        menu.RaiseEvent(new RoutedEventArgs(System.Windows.Controls.ContextMenu.OpenedEvent, menu));

        var items = menu.Items.OfType<MenuItem>().ToList();
        menu.Items.Clear(); // detach them, so they can be hosted somewhere else
        return items;
    }

    /// <summary>
    /// What a shell-metadata cell actually reads: <c>assert-metadata sample.png System.Image.Dimensions 64 x 64</c>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Substring, and deliberately: the property system formats a value for display in the machine's
    /// own conventions, so pinning the whole string would make this fail on a differently-configured
    /// Windows rather than on a broken read.
    /// </para>
    /// <para>
    /// <b>Read off the rendered cell, not out of the view model.</b> The first version asked the row
    /// in C# and passed while every such column rendered permanently blank — the values were being
    /// read and cached, and the binding that should have shown them was never updating. An assertion
    /// that cannot see that is not asserting the feature.
    /// </para>
    /// </remarks>
    private void AssertMetadata(string rest)
    {
        var (name, tail) = Split(rest);
        var (canonical, expected) = Split(tail);
        if (canonical.Length == 0 || expected.Length == 0)
            throw new FormatException("assert-metadata wants a row, a canonical name and the text to find.");

        var row = Row(name);
        var actual = session.Dispatcher.Invoke(() => CellText(row, canonical));

        if (!actual.Contains(expected.Trim('"'), StringComparison.OrdinalIgnoreCase))
            throw new AssertionException(
                $"the '{canonical}' cell of '{name}' reads '{actual}', expected it to contain '{expected}'.");
    }

    /// <summary>What a row's cell for one column actually has on screen.</summary>
    private string CellText(FileItemViewModel row, string columnId)
    {
        var ids = LiveColumns().Select(FileListColumns.GetId).ToList();
        var index = ids.FindIndex(id => id.Equals(columnId, StringComparison.OrdinalIgnoreCase));
        if (index < 0)
            throw new AssertionException($"there is no '{columnId}' column: {ColumnSummary()}");

        if (FileList().ItemContainerGenerator.ContainerFromItem(row) is not ListViewItem container)
            throw new AssertionException(
                $"'{row.Name}' has no realized row, so nothing is rendered to read.");

        var presenter = VisualTreeUtil.FindDescendant<GridViewRowPresenter>(container)
            ?? throw new AssertionException("the row has no GridViewRowPresenter.");

        // Ordered by where each cell actually sits, not by the order GridViewRowPresenter happens to
        // hold its children in — those two are not the same, and assuming they were made this read
        // the neighbouring column.
        var cells = Enumerable.Range(0, VisualTreeHelper.GetChildrenCount(presenter))
            .Select(i => VisualTreeHelper.GetChild(presenter, i))
            .OfType<UIElement>()
            .OrderBy(c => c.TranslatePoint(new Point(0, 0), presenter).X)
            .ToList();

        var cell = cells.ElementAtOrDefault(index);
        if (cell is null) return "";

        // The cell itself as well as its descendants: a column with a CellTemplate puts a
        // ContentPresenter here with the TextBlock inside it, while one rendered from
        // DisplayMemberBinding — which is every shell-metadata column — *is* the TextBlock. Looking
        // only underneath reads those as permanently empty, which is a very convincing way to
        // mistake a working feature for a broken one.
        var blocks = new[] { cell }.Concat(Descendants(cell)).OfType<TextBlock>();
        return string.Join("", blocks.Select(t => t.Text));
    }

    /// <summary>The whole column order, as ids: <c>assert-columns Name, Size, Type</c>.</summary>
    private void AssertColumns(string rest)
    {
        var wanted = Require(rest, "assert-columns")
            .Split(',', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
        var actual = session.Dispatcher.Invoke(() => LiveColumns().Select(FileListColumns.GetId).ToArray());

        if (!wanted.SequenceEqual(actual, StringComparer.OrdinalIgnoreCase))
            throw new AssertionException(
                $"expected the columns to be [{string.Join(", ", wanted)}], they are [{string.Join(", ", actual)}].");
    }

    private FileItemViewModel Row(string name) => session.Dispatcher.Invoke(() =>
        session.Tab.FileList.Items.FirstOrDefault(
            i => i.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
        ?? throw new AssertionException(
            $"There is no row named '{name}'. The list holds: {string.Join(", ", RowNames())}"));

    private IReadOnlyList<string> RowNames() =>
        session.Dispatcher.Invoke(() => session.Tab.FileList.Items.Select(i => i.Name).ToList());

    private IReadOnlyList<FileItemViewModel> Selection() => session.Dispatcher.Invoke(() =>
        session.Tab.SelectedItems.Count > 0
            ? session.Tab.SelectedItems
            : throw new AssertionException("Nothing is selected; use 'select <name>' first."));

    /// <summary>The selection, or nothing, for the verbs where "nothing selected" is a real answer
    /// rather than a mistake — Extract and Compress both mean the whole folder in that case.</summary>
    private IReadOnlyList<FileItemViewModel> SelectionOrEmpty() =>
        session.Dispatcher.Invoke(() => session.Tab.SelectedItems.ToList());

    // ---- tabs and panes ---------------------------------------------------------------

    private void NewTab(string rest)
    {
        var path = rest.Length == 0 ? session.Tab.CurrentPath : _sandbox.Resolve(rest);
        Invoke(() => session.Shell.ActivePane.AddTab(path, activate: true));
        session.Settle();
    }

    private void ActivateTab(string rest)
    {
        var index = Number(rest, "tab");
        Invoke(() =>
        {
            var tabs = session.Shell.ActivePane.Tabs;
            if (index < 1 || index > tabs.Count)
                throw new AssertionException($"This pane has {tabs.Count} tab(s); there is no tab {index}.");

            session.Shell.ActivePane.ActiveTab = tabs[index - 1];
        });
        session.Settle();
    }

    private void Split(string rest, out PaneViewModel pane)
    {
        var (direction, tail) = Split(rest);

        var orientation = direction.ToLowerInvariant() switch
        {
            "right" => SplitOrientation.Vertical,
            "down" or "below" => SplitOrientation.Horizontal,
            _ => throw new FormatException($"split wants 'right' or 'down', got '{direction}'."),
        };

        var path = tail.Length == 0 ? session.Tab.CurrentPath : _sandbox.Resolve(tail);
        pane = session.Dispatcher.Invoke(() =>
        {
            session.Shell.SplitPane(session.Shell.ActivePane, orientation, path);
            return session.Shell.ActivePane;
        });
        session.Settle();
    }

    private void ActivatePane(string rest)
    {
        var index = Number(rest, "pane");
        Invoke(() =>
        {
            var panes = session.Shell.AllPanes.ToList();
            if (index < 1 || index > panes.Count)
                throw new AssertionException($"There are {panes.Count} pane(s); there is no pane {index}.");

            session.Shell.ActivatePane(panes[index - 1]);
        });
        session.Settle();
    }

    // ---- searching --------------------------------------------------------------------

    /// <summary>
    /// Types a query into one of the two search boxes.
    /// </summary>
    /// <remarks>
    /// The whole-PC box is served from the MFT index, which a run only has if it was started with
    /// <c>--index</c> — so <c>gsearch</c> says that rather than reporting an empty result as a
    /// finding.
    /// </remarks>
    private void Search(string rest, bool global)
    {
        var query = Require(rest, global ? "gsearch" : "search");

        // --index-declined is the exception: there the point *is* that there is no index, and the
        // search still has to answer from the crawl fallback and say so.
        if (global && !options.Index && !options.IndexDeclined)
            throw new InvalidOperationException(
                "Whole-PC search reads the MFT index, which this run did not build. Start the " +
                "harness with --index — from an elevated shell, since the index runs in this " +
                "process rather than through the app's helper, and takes minutes — or use " +
                "'search', which walks the current folder.");

        Invoke(() =>
        {
            if (global) session.Tab.GlobalSearchText = query;
            else session.Tab.SearchText = query;
        });

        session.SettleSearch();
    }

    // ---- acting on the selection ------------------------------------------------------

    /// <summary>
    /// Renames the selection, as the dialog's Rename button does.
    /// </summary>
    /// <remarks>
    /// The dialog itself is not driven: <c>ShowDialog</c> would block this thread's dispatcher and
    /// the run would sit there until the watchdog fired. It plans with the same
    /// <c>ShellViewModel.PlanRename</c> and carries out the result with the same
    /// <c>RenameAsync</c>, so nothing between the typed name and the disk is skipped —
    /// use 'dialog rename' to photograph the dialog itself.
    /// </remarks>
    private void Rename(string rest)
    {
        var pattern = Require(rest, "rename");
        Carry(RenameSources("rename"), RenameRule.Simple(pattern), $"Renaming to '{pattern}'");
    }

    /// <summary>
    /// Renames the selection through the dialog's expanded panel — <c>rename-rule find=IMG_
    /// replace=Holiday template="{base} {n:000}{ext}"</c>.
    /// </summary>
    /// <remarks>
    /// Settings are <c>key=value</c>, quoted when they contain a space: <c>template</c>,
    /// <c>find</c>, <c>replace</c>, <c>regex</c>, <c>matchcase</c>, <c>scope</c>, <c>case</c>,
    /// <c>start</c>, <c>step</c>. It goes through the same <c>PlanRename</c> and <c>RenameAsync</c>
    /// the dialog does, so nothing between the rule and the disk is skipped — use
    /// 'dialog rename-advanced' to photograph the panel itself.
    /// </remarks>
    private void RenameByRule(string rest)
    {
        if (rest.Trim().Length == 0)
            throw new FormatException("rename-rule needs key=value settings.");

        Carry(RenameSources("rename-rule"), ParseRule(rest), "That rule");
    }

    private List<RenameSource> RenameSources(string verb) => Selection()
        .Select(i => new RenameSource(
            _sandbox.RequireInside(i.FullPath, verb), i.IsDirectory, Modified(i)))
        .ToList();

    /// <summary>
    /// Plans and runs an archive edit, failing the script with the refusal rather than silently
    /// doing nothing — the harness's counterpart to the message box the menu shows.
    /// </summary>
    private void EditArchive(IReadOnlyList<BertBrowser.Core.Services.Archives.ArchiveEdit> edits)
    {
        var plan = session.Dispatcher.Invoke(
            () => session.Shell.PlanArchiveEdit(session.Tab, edits));

        if (plan.Rejected is { } rejected)
            throw new AssertionException($"The archive edit was refused: {rejected.Message}");

        var outcome = Await(() => session.Shell.ExecuteArchiveEditAsync(plan));

        if (outcome?.Failure is { } failure) throw new AssertionException(failure);
    }

    private void Carry(List<RenameSource> sources, RenameRule rule, string what)
    {
        // The same routing the rename handler does: one entry, and the container is rewritten.
        if (session.Dispatcher.Invoke(() => session.Tab.FileList.IsInsideArchive))
        {
            var naming = session.Dispatcher.Invoke(() => session.Shell.PlanRenameInArchive(sources, rule));
            if (naming.Renames.Count == 0)
                throw new AssertionException($"{what} would change nothing.");

            var entry = session.Dispatcher.Invoke(
                () => session.Shell.ArchiveEntryPathFor(sources[0].Path)) ?? "";

            EditArchive([new BertBrowser.Core.Services.Archives.RenameEntry(
                entry, naming.Renames[0].TargetName)]);
            return;
        }

        var plan = session.Dispatcher.Invoke(() => session.Shell.PlanRename(sources, rule));

        if (plan.Rejected is { Count: > 0 } rejected)
            throw new AssertionException(
                $"The rename was refused: {string.Join("; ", rejected.Select(r => r.Message))}");

        if (!plan.HasWork) throw new AssertionException($"{what} would change nothing.");

        var outcome = Await(() => session.Shell.RenameAsync(plan));

        // See the note on Delete: a permission failure is what this harness is built to produce.
        if (outcome.Failed is { Count: > 0 } failures && !failures.All(f => f.AccessDenied))
            throw new AssertionException(string.Join("; ", failures.Select(f => f.Message)));
    }

    /// <summary>The row's modified time as the dialog hands it over: local, and null when the row
    /// has never been hydrated.</summary>
    private static DateTime? Modified(FileItemViewModel item) =>
        item.ModifiedUtc == default ? null : item.ModifiedUtc.ToLocalTime();

    private static RenameRule ParseRule(string rest)
    {
        var rule = new RenameRule("{name}");

        foreach (var (key, value) in Settings(rest))
            rule = key switch
            {
                "template" => rule with { Template = value },
                "find" => rule with { Find = value },
                "replace" => rule with { Replace = value },
                "regex" => rule with { UseRegex = Switch(value, "regex") },
                "matchcase" => rule with { MatchCase = Switch(value, "matchcase") },
                "scope" => rule with { Scope = Choice<RenameScope>(value, "scope") },
                "case" => rule with { Case = Choice<RenameCase>(value, "case") },
                "start" => rule with { CounterStart = Number(value, "start") },
                "step" => rule with { CounterStep = Number(value, "step") },
                _ => throw new FormatException(
                    $"'{key}' is not a rename-rule setting. Try template, find, replace, regex, " +
                    "matchcase, scope, case, start, step."),
            };

        return rule;
    }

    private static TEnum Choice<TEnum>(string value, string key) where TEnum : struct, Enum =>
        Enum.TryParse<TEnum>(value, ignoreCase: true, out var parsed)
            ? parsed
            : throw new FormatException(
                $"'{value}' is not a {key}. Try {string.Join(", ", Enum.GetNames<TEnum>())}.");

    /// <summary>Splits <c>key=value key="value with spaces"</c> into pairs. An empty value is a
    /// real setting — <c>replace=</c> is how a find is deleted rather than substituted.</summary>
    private static IEnumerable<(string Key, string Value)> Settings(string text)
    {
        var i = 0;
        while (i < text.Length)
        {
            while (i < text.Length && text[i] == ' ') i++;
            if (i >= text.Length) yield break;

            var equals = text.IndexOf('=', i);
            if (equals < 0)
                throw new FormatException($"rename-rule wants key=value, got '{text[i..].Trim()}'.");

            var key = text[i..equals].Trim().ToLowerInvariant();
            i = equals + 1;

            if (i < text.Length && text[i] == '"')
            {
                var close = text.IndexOf('"', i + 1);
                if (close < 0) throw new FormatException($"rename-rule's {key} has an unclosed quote.");
                yield return (key, text[(i + 1)..close]);
                i = close + 1;
                continue;
            }

            var space = text.IndexOf(' ', i);
            if (space < 0) { yield return (key, text[i..]); yield break; }
            yield return (key, text[i..space]);
            i = space;
        }
    }

    /// <summary>Creates a folder or a file in the active tab's directory, as the New menu does.</summary>
    /// <remarks>
    /// Unlike every other command here this acts on the *folder* rather than the selection, so it
    /// needs nothing selected. It plans with the same <c>ShellViewModel.PlanNewItem</c> and carries
    /// the result out with the same <c>CreateNewItemAsync</c> — including the PendingSelection
    /// hand-off, which is why 'assert-selected 1' after one of these is a real assertion. Use
    /// 'dialog new-folder' / 'dialog new-file' to photograph the dialog itself.
    /// </remarks>
    private void NewItem(string rest, NewItemKind kind)
    {
        var verb = kind == NewItemKind.Folder ? "newfolder" : "newfile";
        var name = Require(rest, verb);

        var directory = session.Dispatcher.Invoke(() => session.Tab.CurrentPath);
        if (directory.Length == 0) throw new AssertionException($"No folder is open to {verb} in.");

        var template = kind == NewItemKind.File ? TemplateFor(name) : null;
        var plan = session.Dispatcher.Invoke(
            () => session.Shell.PlanNewItem(directory, name, kind, template?.TemplatePath));

        if (plan.Rejected is { } rejected)
            throw new AssertionException($"The create was refused: {rejected.Message}");

        _sandbox.RequireInside(plan.TargetPath, verb);

        var outcome = Await(() => session.Shell.CreateNewItemAsync(plan));

        if (outcome.Failed is { } failure) throw new AssertionException(failure.Message);
    }

    /// <summary>The configured type whose extension the name ends with, so 'newfile letter.rtf'
    /// picks up that type's template rather than making an empty file.</summary>
    private NewFileTemplate? TemplateFor(string name) =>
        session.Services.GetRequiredService<AppSettings>().ResolvedNewFileTypes
            .FirstOrDefault(t => name.EndsWith(t.Extension, StringComparison.OrdinalIgnoreCase));

    /// <summary>Deletes the selection, as the confirmation's Delete button does.</summary>
    private void Delete(string rest, DeleteMode mode)
    {
        // Inside a container, delete means rewriting the container without those entries — a
        // different planner and a different executor. Routed here as well as in the menu, so a
        // script exercises the same path a click does rather than the one it bypassed.
        if (session.Dispatcher.Invoke(() => session.Tab.FileList.IsInsideArchive))
        {
            var paths = (rest.Length == 0
                    ? Selection().Select(i => i.FullPath)
                    : rest.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                        .Select(n => Row(n).FullPath))
                .ToList();

            EditArchive(session.Dispatcher.Invoke(() => session.Shell.RemovalsFor(paths)));
            return;
        }

        var sources = (rest.Length == 0
                ? Selection().Select(i => (i.FullPath, i.IsDirectory))
                : rest.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                    .Select(Row).Select(i => (i.FullPath, i.IsDirectory)))
            .Select(i => new DeleteSource(_sandbox.RequireInside(i.FullPath, "delete"), i.IsDirectory))
            .ToList();

        var plan = session.Dispatcher.Invoke(() => session.Shell.PlanDelete(sources, mode));

        if (plan.Problems is { Count: > 0 } problems)
            throw new AssertionException(
                $"The delete was refused: {string.Join("; ", problems.Select(p => p.Message))}");

        if (!plan.HasWork) throw new AssertionException("There was nothing to delete.");

        var outcome = Await(() => session.Shell.DeleteAsync(plan));

        // A permission failure is not a script error here. This run never starts the elevated helper,
        // so an item Windows refused is *expected* to come back failed — that is the harness working
        // as designed, and assert-elevation-offered is how a script says it meant to produce one.
        // Everything else still stops the run.
        if (outcome.Failed is { Count: > 0 } failures && !failures.All(f => f.AccessDenied))
            throw new AssertionException(string.Join("; ", failures.Select(f => f.Message)));
    }

    /// <summary>
    /// Moves or copies the selection into a folder, through the drop path.
    /// </summary>
    /// <remarks>
    /// Deliberately not the clipboard. <c>Ctrl+C</c>/<c>Ctrl+V</c> go through the one Windows
    /// clipboard the user is also using, and a script that set it would throw away whatever they
    /// had copied. The paste command is a thin facade over this same planner and executor, so the
    /// code under test is the same either way.
    /// </remarks>
    /// <summary>
    /// Lays down the containers no part of this repository can write: an AES-encrypted zip, a 7z
    /// with encrypted headers, and an ordinary 7z.
    /// </summary>
    /// <remarks>
    /// The bytes come from <c>ArchiveFixtures</c> in Core, the same constants the unit tests use,
    /// so a script and a test can never be looking at different archives. Everything else a script
    /// needs is written with SharpCompress by <c>preview-fixture</c>.
    /// </remarks>
    private void ArchiveFixture(string rest)
    {
        var root = _sandbox.RequireInside(rest.Length == 0 ? "." : rest, "archive-fixture");
        Directory.CreateDirectory(root);

        BertBrowser.Core.Services.Archives.ArchiveFixtures.WriteTo(
            Path.Combine(root, "locked.zip"),
            BertBrowser.Core.Services.Archives.ArchiveFixtures.EncryptedZip);

        BertBrowser.Core.Services.Archives.ArchiveFixtures.WriteTo(
            Path.Combine(root, "sealed.7z"),
            BertBrowser.Core.Services.Archives.ArchiveFixtures.HeaderEncryptedSevenZip);

        BertBrowser.Core.Services.Archives.ArchiveFixtures.WriteTo(
            Path.Combine(root, "plain.7z"),
            BertBrowser.Core.Services.Archives.ArchiveFixtures.PlainSevenZip);

        Console.WriteLine($"# archive fixture: {root}");
    }

    /// <summary>
    /// Pulls entries out of the archive the tab is showing: <c>extract to &lt;folder&gt;</c>, or
    /// <c>extract &lt;names&gt; to &lt;folder&gt;</c> for part of it.
    /// </summary>
    /// <remarks>
    /// Goes through <c>PlanExtract</c>/<c>ExecuteExtractAsync</c>, so a script exercises the same
    /// planner, executor and progress surface the menu does — the reason <c>move</c> and
    /// <c>copy</c> go through the transfer executor rather than the clipboard.
    /// </remarks>
    private void Extract(string rest)
    {
        var (names, destination) = SplitOn(rest, "to", "extract");

        var entries = names.Length == 0
            ? []
            : names.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Select(n => Row(n).FullPath)
                .ToList();

        var target = _sandbox.RequireInside(destination, "extract");
        Directory.CreateDirectory(target);

        var plan = session.Dispatcher.Invoke(
            () => session.Shell.PlanExtract(
                session.Tab, entries, target,
                BertBrowser.Core.Services.Archives.ExtractConflict.KeepBoth));

        if (plan.Rejected is { } rejected)
            throw new AssertionException($"The extract was refused: {rejected.Message}");

        Await(() => session.Shell.ExecuteExtractAsync(plan));
    }

    /// <summary>
    /// Compresses the selection into a new archive: <c>compress zip Backup</c>, or
    /// <c>compress tar.gz Backup</c>. With nothing selected it takes the folder on show.
    /// </summary>
    private void Compress(string rest)
    {
        var (format, name) = SplitFirstWord(Require(rest, "compress"));

        var info = BertBrowser.Core.Services.Archives.ArchiveWriteRules.Formats
            .FirstOrDefault(f => f.Label.Equals(format, StringComparison.OrdinalIgnoreCase))
            ?? throw new FormatException(
                $"'{format}' is not a format this app can write. Try: " +
                string.Join(", ", BertBrowser.Core.Services.Archives.ArchiveWriteRules.Formats
                    .Select(f => f.Label.ToLowerInvariant())));

        if (name.Length == 0) throw new FormatException("compress needs a name for the archive.");

        var selection = Selection();
        var sources = selection.Count > 0
            ? selection.Select(i => _sandbox.RequireInside(i.FullPath, "compress")).ToList()
            : [_sandbox.RequireInside(session.Tab.CurrentPath, "compress")];

        var target = _sandbox.RequireInside(
            Path.Combine(session.Tab.CurrentPath, name + info.Suffix), "compress");

        Await(() => session.Shell.ExecuteCreateArchiveAsync(
            sources, target, info.Format,
            BertBrowser.Core.Services.Archives.CompressionLevel.Normal));
    }

    /// <summary>
    /// Gives the archive the tab is showing a password, and reloads.
    /// </summary>
    /// <remarks>
    /// Writes the session store directly rather than driving the dialog, because the harness never
    /// clicks — the same reason <c>move</c> goes through the transfer executor rather than the
    /// clipboard. What it exercises is the part that matters: the reader consulting the store, the
    /// cache preferring an unlocked read over a locked one, and the banner clearing.
    /// </remarks>
    private void Unlock(string rest)
    {
        var password = Require(rest, "unlock");

        var archive = session.Dispatcher.Invoke(
            () => session.Shell.ArchiveFileFor(session.Tab.CurrentPath))
            ?? throw new AssertionException("This tab is not showing an archive.");

        session.Dispatcher.Invoke(() => session.Shell.RememberArchivePassword(archive, password));
        Await(() => session.Tab.RefreshViewAsync());
    }

    private static (string First, string Remainder) SplitFirstWord(string text)
    {
        var cut = text.IndexOf(' ');
        return cut < 0 ? (text, "") : (text[..cut], text[(cut + 1)..].Trim());
    }

    private void Transfer(string rest, TransferVerb verb)
    {
        var (names, destination) = SplitOn(rest, "to", verb.ToString().ToLowerInvariant());

        var sources = (names.Length == 0
                ? Selection().Select(i => i.FullPath)
                : names.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                    .Select(n => Row(n).FullPath))
            .Select(p => _sandbox.RequireInside(p, verb.ToString()))
            .ToList();

        var target = _sandbox.RequireInside(destination, verb.ToString());
        if (!Directory.Exists(target)) throw new AssertionException($"There is no folder at '{target}'.");

        var plan = session.Dispatcher.Invoke(() => session.Shell.PlanDrop(sources, target, verb));

        if (plan.Problems is { Count: > 0 } problems)
            throw new AssertionException(
                $"The {verb.ToString().ToLowerInvariant()} was refused: " +
                string.Join("; ", problems.Select(p => p.Message)));

        if (!plan.HasWork) throw new AssertionException($"There was nothing to {verb.ToString().ToLowerInvariant()}.");

        Await(() => session.Shell.ExecuteDropAsync(plan, resolutions: null));
    }

    /// <summary>
    /// Poses the transfer-progress surfaces at a fixed point, so the status-bar strip and the
    /// detail window can be photographed.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>Nothing is transferred.</b> A real transfer fast enough to be safe here is over before a
    /// capture could catch it, and one slow enough to catch would put timing — a throughput figure
    /// and a time remaining — into every picture, so no two runs would ever match. The numbers are
    /// therefore given, and the surfaces are the app's own.
    /// </para>
    /// <para>
    /// It deliberately does <em>not</em> set <c>IsTransferring</c>: that is what
    /// <see cref="UiSession.Settle"/> waits on, so a posed transfer would hang the script until the
    /// watchdog fired. Both surfaces bind to the nullable progress view model instead, which is why
    /// this works at all.
    /// </para>
    /// <para>
    /// <c>progress-demo</c> alone poses a plan whose byte total is known; <c>progress-demo unsized</c>
    /// poses one the size index could not total, which is the degraded shape — throughput and bytes
    /// so far, no percentage, no time remaining, and an indeterminate bar.
    /// </para>
    /// </remarks>
    private void ProgressDemo(string rest)
    {
        var argument = rest.Trim();
        if (argument.Equals("off", StringComparison.OrdinalIgnoreCase))
        {
            session.Dispatcher.Invoke(() => session.Shell.TransferProgress = null);
            return;
        }

        var complete = !argument.Equals("unsized", StringComparison.OrdinalIgnoreCase);
        if (argument.Length > 0 && complete && !argument.Equals("sized", StringComparison.OrdinalIgnoreCase))
            throw new FormatException("progress-demo takes nothing, 'sized', 'unsized' or 'off'.");

        var destination = Path.Combine(_sandbox.Root, "Archive");
        PlannedTransfer Item(string name, bool isDirectory) =>
            new(Path.Combine(_sandbox.Root, name), isDirectory, Path.Combine(destination, name), false);

        var plan = new TransferPlan(
            TransferVerb.Copy, destination,
            [Item("archive.zip", false), Item("photo.jpg", false), Item("Pictures", true)],
            []);

        var estimate = new TransferEstimate(50L * 1024 * 1024 * 1024, 1_284, complete);

        session.Dispatcher.Invoke(() =>
        {
            // A cancel that does nothing — there is nothing to stop. Given rather than left null so
            // the button photographs in the state it is really in during a transfer, enabled.
            var surface = new TransferProgressViewModel(plan, estimate, cancel: () => { });
            surface.PoseForCapture(itemsDone: 1, bytesDone: 4_509_715_660, bytesPerSecond: 117_440_512);
            session.Shell.TransferProgress = surface;
        });

        // The strip is only realised once the item it lives in becomes visible, so without this a
        // capture taken straight afterwards gets it measured but not yet filled in — an empty bar
        // and blank labels, which looks like a binding fault and is not one.
        session.Settle();
    }

    private void Undo()
    {
        if (!session.Dispatcher.Invoke(() => session.Shell.CanUndo))
            throw new AssertionException("There is nothing to undo.");

        Invoke(() => session.Shell.UndoCommand.Execute(null));
        session.Settle();
    }


    // --- duplicates ---

    /// <summary>
    /// The duplicates view for this run, kept between commands so a scan, the ticking and the
    /// delete are three script lines rather than one that does everything.
    /// </summary>
    private DuplicatesViewModel? _duplicates;

    /// <summary>
    /// Scans a folder for identical files. <c>duplicates [path]</c>, defaulting to the folder the
    /// active tab is showing.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <b>It crawls the folder into the index first,</b> and has to: a harness run is unelevated,
    /// so <c>MftVolumeIndexer.Open()</c> fails soft on every volume and there are no fs_entry rows
    /// for the sandbox to shortlist from. <c>IndexCrawler</c> writes the same rows with the
    /// same sizes the MFT pass would, so what is scanned is what the app would scan — this is the
    /// fallback path a network share takes, not a special case invented for testing.
    /// </para>
    /// <para>
    /// The scan is awaited rather than left running. <c>UiSession.IsBusy</c> knows about listings
    /// and transfers, not about this, so a bare <c>settle</c> after it would return while the
    /// hashing was still going.
    /// </para>
    /// </remarks>
    private void Duplicates(string rest)
    {
        var path = rest.Length == 0
            ? session.Dispatcher.Invoke(() => session.Tab.CurrentPath)
            : _sandbox.Resolve(rest);

        if (!Directory.Exists(path))
            throw new AssertionException($"There is no folder at '{path}' to search.");

        var crawler = session.Services.GetRequiredService<BertBrowser.Core.Services.IndexCrawler>();
        Await(() => crawler.CrawlAsync(path, CancellationToken.None));

        var view = DuplicatesView();
        Await(() => view.ScanAsync(path));

        if (options.Verbose)
            output.WriteLine($"# {view.Groups.Count} groups, {view.ReclaimableBytes} bytes reclaimable");
    }

    /// <summary>Ticks every copy but one, the way the view's own buttons do.</summary>
    private void DuplicatesKeep(string rest)
    {
        var view = RequireDuplicates("duplicates-keep");

        var strategy = rest.Trim().ToLowerInvariant() switch
        {
            "newest" => KeepStrategy.Newest,
            "oldest" => KeepStrategy.Oldest,
            "shallowest" => KeepStrategy.Shallowest,
            var other => throw new FormatException(
                $"'{other}' is not a keep strategy. Try: newest, oldest, shallowest."),
        };

        session.Dispatcher.Invoke(() =>
        {
            foreach (var group in view.Groups) group.TickAllBut(strategy);
        });

        session.Settle();

        if (options.Verbose)
            output.WriteLine("# groups: " + string.Join(", ", view.Groups.Select(g => $"{g.Files.Count} files/{g.TickedCount} ticked")) + $" total={view.TickedCount}");
    }

    /// <summary>Deletes the ticked copies, through the planner and executor a real one goes through.</summary>
    private void DuplicatesRemove()
    {
        var view = RequireDuplicates("duplicates-remove");

        if (view.TickedCount == 0)
            throw new AssertionException("Nothing is ticked. Run 'duplicates-keep' first.");

        Await(() => view.RemoveTickedCommand.ExecuteAsync(null));
    }

    private void AssertDuplicateGroups(string rest)
    {
        var expected = Number(rest, "assert-duplicate-groups");
        var actual = RequireDuplicates("assert-duplicate-groups").Groups.Count;

        if (actual != expected)
            throw new AssertionException($"Expected {expected} duplicate groups, found {actual}.");
    }

    private void AssertDuplicateSelected(string rest)
    {
        var expected = Number(rest, "assert-duplicate-selected");
        var actual = RequireDuplicates("assert-duplicate-selected").TickedCount;

        if (actual != expected)
            throw new AssertionException($"Expected {expected} copies ticked, found {actual}.");
    }

    /// <summary>Asserts some group does, or does not, hold a copy with this name.</summary>
    private void AssertDuplicateRow(string rest, bool expected)
    {
        var verb = expected ? "assert-duplicate-row" : "assert-no-duplicate-row";
        var name = Require(rest, verb);
        var view = RequireDuplicates(verb);

        var names = view.Groups.SelectMany(g => g.Files).Select(f => f.Item.Name).ToList();
        var found = names.Any(n => string.Equals(n, name, StringComparison.OrdinalIgnoreCase));

        if (found != expected)
            throw new AssertionException(expected
                ? $"No duplicate named '{name}'. Found: {string.Join(", ", names.Distinct())}"
                : $"'{name}' was reported as a duplicate, and should not have been.");
    }

    private DuplicatesViewModel RequireDuplicates(string verb) =>
        _duplicates ?? throw new AssertionException($"Run 'duplicates' before '{verb}'.");

    /// <summary>
    /// The view model, made once per run.
    /// </summary>
    /// <remarks>
    /// Its remover goes straight through the planner and executor rather than through
    /// <c>DeleteDialog</c>: the dialog is modal, and <c>ShowDialog</c> would run a nested message
    /// loop on the script's own thread and hang the run until the watchdog fired. Every path is
    /// still fenced to the sandbox first, since this drives the real delete executor.
    /// </remarks>
    private DuplicatesViewModel DuplicatesView()
    {
        if (_duplicates is { } existing) return existing;

        return _duplicates = session.Dispatcher.Invoke(() => new DuplicatesViewModel(
            session.Services.GetRequiredService<IDuplicateFinder>(),
            session.Services.GetRequiredService<IMftIndexService>(),
            RemoveDuplicatesFenced,
            includeHidden: session.Services.GetRequiredService<AppSettings>().ShowHiddenItems,

            // The sandbox's files are a few hundred bytes each, so the shipped one-megabyte floor
            // would shortlist nothing at all.
            minSizeBytes: 1,
            skipSystemFolders: false));
    }

    private async Task<IReadOnlyCollection<string>> RemoveDuplicatesFenced(IReadOnlyList<string> paths)
    {
        var sources = paths
            .Select(p => new DeleteSource(_sandbox.RequireInside(p, "duplicates-remove"), false))
            .ToList();

        var plan = session.Shell.PlanDelete(sources, DeleteMode.Recycle);

        if (plan.Problems is { Count: > 0 } problems)
            throw new AssertionException(
                $"The delete was refused: {string.Join("; ", problems.Select(p => p.Message))}");

        if (!plan.HasWork) return [];

        var outcome = await session.Shell.DeleteAsync(plan);

        if (outcome.Failed is { Count: > 0 } failures)
            throw new AssertionException(string.Join("; ", failures.Select(f => f.Message)));

        return [.. outcome.Deleted.Select(d => d.SourcePath)];
    }
    // ---- browse settings --------------------------------------------------------------

    private void Hidden(string rest) => Invoke(() =>
    {
        session.Shell.ShowHiddenItems = Switch(rest, "hidden");
        session.Settle();
    });

    private void Thumbnails(string rest)
    {
        var scale = Fraction(rest, "thumbnails");
        Invoke(() => session.Tab.FileList.ThumbnailScale = scale);
        session.Settle(quietMs: 120); // tiles decode their icons off-thread
    }

    /// <summary>Shows or hides the active tab's preview pane, then waits for it.</summary>
    /// <remarks>
    /// The quiet period is longer than the thumbnails one and has to be: the preview debounces the
    /// selection for 150 ms before it reads anything, and only then goes off-thread to decode. It
    /// is also why every preview assertion in a script belongs after this command rather than
    /// immediately after a <c>select</c>.
    ///
    /// Nothing here ever starts a media pipeline. The pane shows a poster frame until someone
    /// presses play, and no script presses play — a `MediaElement` renders through its own
    /// composition surface and would come back as a hole in a `RenderTargetBitmap` anyway, quite
    /// apart from making a noise on the machine someone is using.
    /// </remarks>
    private void Preview(string rest)
    {
        var show = Switch(rest, "preview");
        Invoke(() => session.Tab.IsPreviewVisible = show);
        session.Settle(quietMs: 400);
    }

    /// <summary>The pane's Auto / Raw / Hex override. Spelled the way the buttons are, not the way
    /// the enum is — <c>PreviewMode.Text</c> is "raw" on screen.</summary>
    private void PreviewViewMode(string rest)
    {
        var mode = Require(rest, "preview-mode").ToLowerInvariant() switch
        {
            "auto" => PreviewMode.Auto,
            "raw" or "text" => PreviewMode.Text,
            "hex" => PreviewMode.Hex,
            var other => throw new FormatException(
                $"preview-mode wants auto, raw or hex, got '{other}'."),
        };

        Invoke(() => session.Tab.Preview.Mode = mode);

        // The pane re-reads off-thread behind its 150 ms debounce, so an assertion made straight
        // after this command would pass whatever the mode did.
        session.Settle(quietMs: 400);
    }

    private void Sort(string rest)
    {
        var wanted = Require(rest, "sort");

        // Any catalogue id, so a shell-metadata column can be sorted by from a script the same way a
        // built-in can. "date" stays as an alias for Modified; scripts have used it since before
        // columns were configurable.
        var column = wanted.Equals("date", StringComparison.OrdinalIgnoreCase)
            ? ColumnCatalog.Modified
            : ColumnCatalog.BuiltIns.Concat(ColumnCatalog.Curated)
                  .FirstOrDefault(s => s.Id.Equals(wanted, StringComparison.OrdinalIgnoreCase))?.Id
              ?? throw new FormatException(
                  $"sort wants a column id, got '{wanted}'. Built-ins: " +
                  string.Join(", ", ColumnCatalog.BuiltIns.Where(s => s.Sortable).Select(s => s.Id)) + ".");

        Invoke(() => session.Tab.FileList.SetSort(column));
        session.Settle();
    }

    private void Theme(string rest)
    {
        var id = Require(rest, "theme");
        var themes = session.Services.GetRequiredService<IThemeService>();

        if (themes.Available.All(t => !t.Id.Equals(id, StringComparison.OrdinalIgnoreCase)))
            throw new AssertionException(
                $"There is no theme '{id}'. Available: {string.Join(", ", themes.Available.Select(t => t.Id))}");

        Invoke(() => themes.SelectTheme(id));
        session.Settle();
    }

    // ---- capturing --------------------------------------------------------------------

    private void Shot(string rest)
    {
        var (name, elementName) = Split(rest);
        var path = Resolve(Named(name, ++_shots));

        var (width, height) = session.Dispatcher.Invoke(() =>
        {
            FrameworkElement element = elementName.Length == 0
                ? session.Window
                : FindNamed<FrameworkElement>(elementName)
                  ?? throw new FormatException($"There is no element named '{elementName}' in the window.");

            return Capture.Save(element, path);
        });

        if (!Capture.HasContent(path))
            throw new AssertionException(
                $"{path} is a single flat colour — the window rendered nothing.");

        output.WriteLine($"SHOT {path}");
        if (options.Verbose)
            output.WriteLine($"# {width}x{height}{(elementName.Length == 0 ? "" : $" of {elementName}")}");
    }

    /// <summary>
    /// Photographs a dialog.
    /// </summary>
    /// <remarks>
    /// Built and shown modelessly, parked offscreen like its owner. Not <c>ShowDialog</c>: that
    /// runs a nested message loop on this very thread, so the script would stop until the watchdog
    /// killed it — and none of these dialogs is a thing the harness needs to *answer*, only to
    /// look at. Each is constructed through the same factory the app's own entry point uses, so a
    /// capture cannot drift from what the app puts on screen.
    /// </remarks>
    private void Dialog(string rest)
    {
        var (kind, tail) = Split(rest);
        var (name, _) = Split(tail);

        var window = session.Dispatcher.Invoke(() => Build(kind.ToLowerInvariant()));

        try
        {
            session.Dispatcher.Invoke(() =>
            {
                UiSession.Park(window, options, sizeIt: false);
                window.Owner = session.Window;
                window.Show();
            });

            // Long enough for the delete survey to have counted the tree it is describing; the
            // other dialogs are settled by the first pass.
            session.Settle(quietMs: 250);

            var path = Resolve(Named(name.Length == 0 ? $"dialog-{kind}" : name, ++_shots));
            var (width, height) = session.Dispatcher.Invoke(() =>
            {
                window.UpdateLayout();
                return Capture.Save(window, path);
            });

            if (options.Verbose) output.WriteLine($"# {width}x{height} of the {kind} dialog");

            if (!Capture.HasContent(path))
                throw new AssertionException($"{path} is a single flat colour — the dialog rendered nothing.");

            output.WriteLine($"SHOT {path}");
        }
        finally
        {
            session.Dispatcher.Invoke(window.Close);
        }
    }

    private SettingsViewModel SettingsFor(SettingsCategory category)
    {
        var vm = new SettingsViewModel(
            session.Services.GetRequiredService<AppSettings>(),
            session.Services.GetRequiredService<IThemeService>(),
            session.Services.GetRequiredService<IShellNewCatalog>(),
            session.Services.GetRequiredService<IFolderHandlerService>());
        vm.SelectedCategory = vm.Categories.First(c => c.Id == category);
        return vm;
    }

    private Window Build(string kind) => kind switch
    {
        "rename" => RenameDialog.Create(
            Selection().Select(i => new RenameSource(i.FullPath, i.IsDirectory, Modified(i))).ToList(),
            session.Shell.PlanRename),

        // The same dialog with its options panel already open, which is the only way to photograph
        // it: the panel is opened by a click, and the harness never clicks.
        "rename-advanced" => RenameDialog.Create(
            Selection().Select(i => new RenameSource(i.FullPath, i.IsDirectory, Modified(i))).ToList(),
            session.Shell.PlanRename, expanded: true),

        "delete" => DeleteDialog.Create(
            DeletePlanFor(DeleteMode.Recycle), session.Shell.SurveyDelete),

        "delete-permanent" => DeleteDialog.Create(
            DeletePlanFor(DeleteMode.Permanent), session.Shell.SurveyDelete),

        // Posed at a fixed offer rather than provoked by a real refusal: producing a genuine
        // access-denied failure needs an ACL fixture, and what this window is for is being looked
        // at. The wiring behind it is proved by assert-elevation-offered instead.
        "elevation" => ElevationDialog.Create(new ElevationOffer(
            ElevationOperation.Delete,
            [.. Selection().Select(i => i.FullPath)])),

        // Needs no selection and no fixture: its content is SearchSyntax.Sections, which is a
        // constant in Core.
        "search-syntax" => SearchSyntaxDialog.Create(),

        "message" => MessageDialog.Create(
            "The harness built this dialog to photograph it. Nothing went wrong.",
            "Message", MessageDialogKind.Information),

        "warning" => MessageDialog.Create(
            "The harness built this dialog to photograph it. Nothing went wrong.",
            "Warning", MessageDialogKind.Warning, showCancel: true),

        "properties" => new PropertiesDialog(new PropertiesViewModel(
            Selection().Select(i => new PropertiesTarget(i.FullPath, i.IsDirectory)).ToList(),
            session.Services.GetRequiredService<DirSizeRepository>())),

        // Neither of these needs a selection, unlike every other kind here: New acts on the
        // folder being shown.
        "new-folder" => NewItemDialogFor(NewItemKind.Folder),

        "new-file" => NewItemDialogFor(NewItemKind.File),

        "settings" => new SettingsWindow(SettingsFor(SettingsCategory.General)),

        // Settings opens on General, so a page further down the list needs its own kind to be
        // photographed at all.
        "settings-columns" => new SettingsWindow(SettingsFor(SettingsCategory.Columns)),

        "columns" => ColumnPickerDialog.Create(
            session.Window, session.Tab.FileList.ResolvedColumns
                .Where(c => !c.Injected)
                .Select(c => new ColumnSetting(c.Id, c.Width))
                .ToList()),

        "theme-editor" => new ThemeEditorWindow(new AppearanceViewModel(
            session.Services.GetRequiredService<IThemeService>())),

        "disk-usage" => DiskUsageWindowFor(),

        // Needs a 'duplicates' first, so the window shows the results that run found rather than
        // starting a scan of its own while a capture waits on it.
        "duplicates" => _duplicates is { } view
            ? DuplicatesWindow.Create(view, (_, _) => { })
            : throw new AssertionException(
                "There are no duplicate results to show. Run 'duplicates' before 'dialog duplicates'."),

        // Posed rather than run, for the reasons on ProgressDemo. Needs a progress-demo first, so
        // the window shows the same fixed figures the status bar does.
        "transfer" => session.Shell.TransferProgress is { } progress
            ? TransferProgressWindow.Create(progress)
            : throw new AssertionException(
                "There is no transfer to show. Run 'progress-demo' before 'dialog transfer'."),

        // Both are built through the same internal Create the menu goes through, so a capture
        // cannot drift from what the app puts on screen.
        "extract" => ExtractDialog.Create(
            null,
            Path.GetFileName(session.Tab.CurrentPath),
            Path.Combine(session.Tab.CurrentPath, "Unpacked"),
            SelectionOrEmpty().Count),

        "archive-password" => ArchivePasswordDialog.Create(
            null, Path.GetFileName(session.Tab.CurrentPath), retry: false),

        "compress" => CreateArchiveDialog.Create(
            null,
            session.Tab.CurrentPath,
            BertBrowser.Core.Services.Archives.ArchiveWriteRules.SuggestName(
                SelectionOrEmpty().Select(i => i.FullPath).ToList(), session.Tab.CurrentPath),
            Math.Max(1, SelectionOrEmpty().Count)),

        _ => throw new FormatException(
            $"'{kind}' is not a dialog. Try: new-folder, new-file, rename, rename-advanced, " +
            "delete, delete-permanent, message, warning, properties, settings, theme-editor, " +
            "disk-usage, duplicates, transfer, extract, compress, archive-password, " +
            "settings-columns, columns."),
    };

    /// <summary>
    /// The disk-usage view on the folder being shown.
    /// </summary>
    /// <remarks>
    /// A harness run is unelevated, so <c>MftVolumeIndexer.Open()</c> fails soft on every volume and
    /// there are no dir_size_cache rows to read — which means this can only ever photograph the
    /// <em>unknown</em> states. That is deliberate coverage rather than a shortcoming: those are
    /// precisely the states that regress into rendering zeros, and what a populated one looks like
    /// is settled by the Core tests instead.
    /// </remarks>
    private Window DiskUsageWindowFor()
    {
        var vm = new DiskUsageViewModel(
            session.Services.GetRequiredService<IDiskUsageService>(),
            session.Services.GetRequiredService<IMftIndexService>(),
            session.Services.GetRequiredService<AppSettings>().ShowHiddenItems);

        // Revealing would navigate the window behind this one, which a capture has no use for.
        var window = DiskUsageWindow.Create(vm, (_, _) => { });
        window.Load(session.Tab.CurrentPath);
        return window;
    }

    /// <summary>The New dialog as the menu opens it, suggested name and all.</summary>
    private Window NewItemDialogFor(NewItemKind kind)
    {
        var directory = session.Tab.CurrentPath;
        var template = kind == NewItemKind.File
            ? session.Services.GetRequiredService<AppSettings>().ResolvedNewFileTypes.FirstOrDefault()
            : null;

        return NewItemDialog.Create(
            directory,
            kind,
            template,
            session.Shell.SuggestNewItemName(directory, kind, template),
            session.Shell.PlanNewItem);
    }

    private DeletePlan DeletePlanFor(DeleteMode mode)
    {
        var sources = Selection()
            .Select(i => new DeleteSource(i.FullPath, i.IsDirectory))
            .ToList();

        return session.Shell.PlanDelete(sources, mode);
    }

    private static string Named(string name, int ordinal)
    {
        if (name.Length == 0) return $"{ordinal:00}-shot.png";

        return name.EndsWith(".png", StringComparison.OrdinalIgnoreCase) ? name : name + ".png";
    }

    // ---- reading back -----------------------------------------------------------------

    /// <summary>
    /// Everything worth asserting about, on one line.
    /// </summary>
    /// <remarks>
    /// JSON because a run is usually read by whatever asked for it, and one line because a script's
    /// output is read as a transcript where a pretty-printed object would bury the commands around
    /// it.
    /// </remarks>
    /// <summary>
    /// Saves the current arrangement the way closing the window does, then reopens it in place —
    /// the whole session round trip, against the real shell.
    /// </summary>
    /// <remarks>
    /// Doing both halves in one command is what makes it an assertion rather than a dump: the panes
    /// and tabs that come back can be checked with the ordinary <c>assert-panes</c> and
    /// <c>assert-tabs</c>, so a rebuild that quietly loses one shows up. The saved JSON is printed
    /// too, since that is what lands in settings.json.
    /// </remarks>
    private void Session()
    {
        var saved = session.Dispatcher.Invoke(() => session.Shell.CaptureLayout());

        // What a launch does with it: drop anything no longer on disk before rebuilding.
        var pruned = SessionLayoutRules.Prune(saved, Directory.Exists);
        output.WriteLine("SESSION " + JsonSerializer.Serialize(pruned));

        if (pruned is null)
            throw new AssertionException("The captured session pruned away to nothing.");

        // Await, not Invoke().Result: the continuations want the UI thread, so blocking this one on
        // them is a deadlock. Await pumps instead.
        if (!Await(() => session.Shell.RestoreSessionAsync(pruned)))
            throw new AssertionException("The captured session would not restore.");
    }

    private string State() => session.Dispatcher.Invoke(() =>
    {
        var shell = session.Shell;
        var tab = shell.ActiveTab;

        var fields = new (string Name, string Value)[]
        {
            ("path", Quote(tab.CurrentPath)),
            ("items", Text(tab.FileList.Items.Count)),
            ("selected", Text(tab.SelectedItems.Count)),
            ("flattened", Bool(tab.FileList.IsFlattened)),
            ("columns", Quote(string.Join(", ", tab.FileList.ResolvedColumns.Select(c => c.Id)))),
            ("insideArchive", Bool(tab.FileList.IsInsideArchive)),
            ("search", Quote(tab.ActiveSearchText)),
            ("globalSearch", Bool(tab.IsGlobalSearch)),
            ("tabs", Text(shell.ActivePane.Tabs.Count)),
            ("panes", Text(shell.AllPanes.Count())),
            ("hidden", Bool(shell.ShowHiddenItems)),
            ("thumbnails", tab.FileList.ThumbnailScale.ToString("0.##", CultureInfo.InvariantCulture)),
            ("preview", Bool(tab.IsPreviewVisible)),
            ("previewState", Quote(tab.Preview.StateName)),
            ("previewMode", Quote(tab.Preview.Mode.ToString())),
            ("previewTitle", Quote(tab.Preview.Title)),
            ("previewMessage", Quote(tab.Preview.Message ?? "")),
            ("previewFooter", Quote(tab.Preview.TextFooter)),
            ("previewMetadata", Text(tab.Preview.Metadata.Count)),
            ("canUndo", Bool(shell.CanUndo)),
            ("undo", Quote(shell.UndoDescription)),
            ("foregroundCorrections", Text(session.ForegroundCorrections)),
            ("selection", Quote(tab.SelectionSummary)),
            ("status", Quote(tab.StatusText)),
            ("indexing", Quote(shell.IndexingStatus)),
            ("indexingCanRetry", Bool(shell.IndexingCanRetry)),
            ("isTransferring", Bool(shell.IsTransferring)),
            // The byte-level surface, or nulls when nothing is running. Reported because there was
            // previously no way for a script to see a transfer at all.
            ("transferHeadline", Quote(shell.TransferProgress?.Headline ?? "")),
            ("transferDetail", Quote(shell.TransferProgress?.DetailText ?? "")),
            ("transferBytes", Text(shell.TransferProgress?.BytesDone ?? 0)),
            ("transferBytesTotal", Text(shell.TransferProgress?.BytesTotal ?? 0)),
            ("transferIndeterminate", Bool(shell.TransferProgress?.IsIndeterminate ?? false)),
        };

        return "{" + string.Join(",", fields.Select(f => $"{Quote(f.Name)}:{f.Value}")) + "}";
    });

    /// <summary>
    /// Where a token's colour actually came out, at every level that could have got it wrong.
    /// </summary>
    /// <remarks>
    /// Theming is recolour-in-place: one brush per token, shared by every consumer, its
    /// <c>Color</c> driven by a binding. When that goes wrong the symptom is a window that is half
    /// one theme and half another, and the question is always the same — did the resolver produce
    /// the new colour, did the brush follow it, and is the element still pointing at that brush.
    /// This answers all three at once.
    /// </remarks>
    private void Probe(string rest)
    {
        var (token, elementName) = Split(Require(rest, "probe"));

        output.WriteLine(session.Dispatcher.Invoke(() =>
        {
            var themes = session.Services.GetRequiredService<IThemeService>();
            var app = Application.Current.TryFindResource(token) as SolidColorBrush;
            var window = session.Window.TryFindResource(token) as SolidColorBrush;

            var text = $"PROBE {token} resolver={themes.GetColor(token).ToHex()} " +
                $"app={Describe(app)} window={Describe(window)}";

            if (elementName.Length > 0 && FindNamed<FrameworkElement>(elementName) is { } element)
            {
                var background = element.GetValue(Control.BackgroundProperty) as SolidColorBrush;
                var foreground = element.GetValue(Control.ForegroundProperty) as SolidColorBrush;
                text += $" | {elementName}.Background={Describe(background)} " +
                    $".Foreground={Describe(foreground)}";
            }

            return text;
        }));
    }

    private static string Describe(SolidColorBrush? brush) =>
        brush is null ? "<none>" : $"{brush.Color}{(brush.IsFrozen ? " FROZEN" : "")}#{brush.GetHashCode():X}";

    private static string Text(long value) => value.ToString(CultureInfo.InvariantCulture);

    private static string Bool(bool value) => value ? "true" : "false";

    private static string Quote(string text)
    {
        var escaped = new StringBuilder("\"");
        foreach (var c in text)
        {
            escaped.Append(c switch
            {
                '\\' => "\\\\",
                '"' => "\\\"",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                _ => c.ToString(),
            });
        }

        return escaped.Append('"').ToString();
    }

    // ---- assertions -------------------------------------------------------------------

    private void AssertPath(string rest)
    {
        var expected = Require(rest, "assert-path");
        var actual = session.Dispatcher.Invoke(() => session.Tab.CurrentPath);

        if (!actual.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the path to contain '{expected}', got '{actual}'.");
    }

    /// <summary>Asserts on the running transfer's own strip — its headline and its figures — which
    /// is a different line from <c>assert-status</c>'s.</summary>
    private void AssertTransfer(string rest)
    {
        var expected = Require(rest, "assert-transfer");
        var actual = session.Dispatcher.Invoke(() => session.Shell.TransferProgress is { } p
            ? $"{p.Headline}  {p.DetailText}"
            : null);

        if (actual is null)
            throw new AssertionException(
                $"expected a transfer showing '{expected}', but none is running. Run 'progress-demo' first.");

        if (!actual.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the transfer to show '{expected}', got '{actual}'.");
    }

    /// <summary>
    /// Asserts the transfer bar is indeterminate — which is what a plan the size index could not
    /// total has to look like. A determinate bar there would sit at zero and read as a stall.
    /// </summary>
    private void AssertTransferIndeterminate()
    {
        var indeterminate = session.Dispatcher.Invoke(() => session.Shell.TransferProgress?.IsIndeterminate);

        if (indeterminate is null)
            throw new AssertionException("no transfer is running. Run 'progress-demo unsized' first.");
        if (indeterminate is false)
            throw new AssertionException(
                "the transfer bar is determinate, but its byte total was never established — " +
                "a bar pinned at zero reads as a stall rather than as an unmeasured volume.");
    }

    private void AssertStatus(string rest)
    {
        var expected = Require(rest, "assert-status");
        var actual = session.Dispatcher.Invoke(() => session.Tab.StatusText);

        if (!actual.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the status to contain '{expected}', got '{actual}'.");
    }

    /// <summary>
    /// Asserts on the warning banner above the list — <c>FileList.ErrorMessage</c>. With no
    /// argument, asserts there is no banner at all.
    /// </summary>
    /// <remarks>
    /// Separate from <c>assert-status</c>: the status line always says something, so a check
    /// against it cannot tell "the query was refused" from "the query ran and found nothing".
    /// The banner appears only for the first.
    /// </remarks>
    /// <summary>
    /// Asserts on a row's Match column: <c>assert-match &lt;row&gt; &lt;text&gt;</c>.
    /// </summary>
    /// <remarks>
    /// The only way to see the matching line through the real window. Without it the whole
    /// content-snippet path — the reader's line numbering, the clipping, and the three runs the
    /// cell is built from — is covered by unit tests and by nothing that renders.
    /// </remarks>
    private void AssertMatch(string rest)
    {
        var parts = rest.Split(' ', 2, StringSplitOptions.TrimEntries);
        if (parts.Length < 2)
            throw new InvalidOperationException("assert-match needs a row name and some text.");

        var (row, expected) = (parts[0], parts[1]);

        var actual = session.Dispatcher.Invoke(() =>
            session.Tab.FileList.Items
                .FirstOrDefault(i => string.Equals(i.Name, row, StringComparison.OrdinalIgnoreCase))
                ?.Match?.Line);

        if (actual is null)
            throw new AssertionException(
                $"expected row '{row}' to carry a content match containing '{expected}', " +
                "but it has none (or the row is not there).");

        if (!actual.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException(
                $"expected '{row}' to have matched on a line containing '{expected}', got '{actual}'.");
    }

    private void AssertError(string rest)
    {
        var actual = session.Dispatcher.Invoke(() => session.Tab.FileList.ErrorMessage);

        if (string.IsNullOrWhiteSpace(rest))
        {
            if (actual is not null)
                throw new AssertionException($"expected no error banner, got '{actual}'.");
            return;
        }

        if (actual is null)
            throw new AssertionException($"expected an error banner containing '{rest}', there was none.");
        if (!actual.Contains(rest, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the error banner to contain '{rest}', got '{actual}'.");
    }

    /// <summary>
    /// Asserts on the indexing line in the status bar, and on whether it offers a retry.
    /// </summary>
    /// <remarks>
    /// Separate from <c>assert-status</c> because it is the shell's, not the tab's: it describes
    /// the machine's index rather than what one directory listing is doing. <c>retry</c> and
    /// <c>no-retry</c> assert the affordance, which is the difference between "this will fix
    /// itself" and "this needs you".
    /// </remarks>
    private void AssertIndexing(string rest)
    {
        var expected = Require(rest, "assert-indexing");
        var (status, canRetry) = session.Dispatcher.Invoke(
            () => (session.Shell.IndexingStatus, session.Shell.IndexingCanRetry));

        switch (expected)
        {
            case "retry" when !canRetry:
                throw new AssertionException($"expected a retry to be offered; the index line is '{status}'.");
            case "no-retry" when canRetry:
                throw new AssertionException($"expected no retry to be offered; the index line is '{status}'.");
            case "retry" or "no-retry":
                return;
        }

        if (!status.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the index line to contain '{expected}', got '{status}'.");
    }

    private void AssertCount(string rest)
    {
        var expected = Number(rest, "assert-count");
        var actual = session.Dispatcher.Invoke(() => session.Tab.FileList.Items.Count);

        if (actual != expected)
            throw new AssertionException(
                $"expected {expected} row(s), got {actual}: {string.Join(", ", RowNames())}");
    }

    private void AssertRow(string rest, bool expected)
    {
        var name = Require(rest, expected ? "assert-row" : "assert-no-row");
        var present = RowNames().Any(n => n.Equals(name, StringComparison.OrdinalIgnoreCase));

        if (present != expected)
            throw new AssertionException(expected
                ? $"'{name}' is not in the list: {string.Join(", ", RowNames())}"
                : $"'{name}' is still in the list.");
    }

    private void AssertSelected(string rest)
    {
        var expected = Number(rest, "assert-selected");
        var actual = session.Dispatcher.Invoke(() => session.Tab.SelectedItems.Count);

        if (actual != expected)
            throw new AssertionException($"expected {expected} selected item(s), got {actual}.");
    }

    /// <summary>What the preview pane settled on: image, document, text, archive, font, media,
    /// loading, or none. Asserted through the view model rather than a screenshot, because the
    /// picture of a poster frame proves nothing about which branch produced it.</summary>
    private void AssertPreview(string rest)
    {
        var expected = Require(rest, "assert-preview").ToLowerInvariant();
        var actual = session.Dispatcher.Invoke(() => session.Tab.Preview.StateName);

        if (!string.Equals(actual, expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the preview to be '{expected}', it is '{actual}'.");
    }

    private void AssertTabs(string rest)
    {
        var expected = Number(rest, "assert-tabs");
        var actual = session.Dispatcher.Invoke(() => session.Shell.ActivePane.Tabs.Count);

        if (actual != expected)
            throw new AssertionException($"expected {expected} tab(s) in this pane, got {actual}.");
    }

    private void AssertPanes(string rest)
    {
        var expected = Number(rest, "assert-panes");
        var actual = session.Dispatcher.Invoke(() => session.Shell.AllPanes.Count());

        if (actual != expected)
            throw new AssertionException($"expected {expected} pane(s), got {actual}.");
    }

    private void AssertFlattened(bool expected)
    {
        var actual = session.Dispatcher.Invoke(() => session.Tab.FileList.IsFlattened);

        if (actual != expected)
            throw new AssertionException(expected
                ? "the list is a normal directory listing, not a flattened search result."
                : "the list is still a flattened search result.");
    }

    /// <summary>
    /// Whether the list is showing the inside of a container.
    /// </summary>
    /// <remarks>
    /// The counterpart to <c>assert-flattened</c>, and it exists for the same reason: this is the
    /// flag every write verb is gated on, so without a way to assert it a script can only check
    /// that Rename did nothing — which it would also do if the guard had been removed and the
    /// rename simply failed.
    /// </remarks>
    private void AssertInsideArchive(bool expected)
    {
        var actual = session.Dispatcher.Invoke(() => session.Tab.FileList.IsInsideArchive);

        if (actual != expected)
            throw new AssertionException(expected
                ? "the list is a folder on disk, not the inside of an archive."
                : "the list is still showing the inside of an archive.");
    }

    private void AssertCanUndo(bool expected)
    {
        var actual = session.Dispatcher.Invoke(() => session.Shell.CanUndo);

        if (actual != expected)
            throw new AssertionException(expected
                ? "there is nothing to undo."
                : "there is still something to undo.");
    }

    private void AssertOnDisk(string rest, bool expected)
    {
        var path = _sandbox.Resolve(Require(rest, expected ? "assert-exists" : "assert-missing"));
        var present = File.Exists(path) || Directory.Exists(path);

        if (present != expected)
            throw new AssertionException(expected
                ? $"there is nothing at '{path}'."
                : $"'{path}' is still there.");
    }

    private void AssertVisibility(string rest, bool expected)
    {
        var name = Require(rest, expected ? "assert-visible" : "assert-hidden");

        var actual = session.Dispatcher.Invoke(() =>
            FindNamed<FrameworkElement>(name)?.Visibility == Visibility.Visible);

        if (actual != expected)
            throw new AssertionException(
                $"expected {name} to be {(expected ? "visible" : "not visible")}, it is not.");
    }

    /// <summary>
    /// Asserts this run started no programs.
    /// </summary>
    /// <remarks>
    /// The launcher refuses regardless, so this is not a safety net — it is how a script proves a
    /// gesture it drove did not even <em>try</em> to put a window on the user's desktop.
    /// </remarks>
    private void AssertNotLaunched()
    {
        if (session.Launcher.Attempts is { Count: > 0 } attempts)
            throw new AssertionException(
                $"the run tried to start: {string.Join(", ", attempts)} (all refused).");
    }

    /// <summary>
    /// Whether the run has offered to retry something as administrator.
    /// </summary>
    /// <remarks>
    /// The negative form is the more valuable one. A file that is merely read-only, or open in
    /// something else, fails with an exception that looks a great deal like a permission problem
    /// from a distance and is not one — and putting a UAC prompt in front of either would be a
    /// regression no unit test spans, because the discriminator and the dialog sit on opposite
    /// sides of the view model.
    /// </remarks>
    private void AssertElevationOffered(bool expected)
    {
        var offers = session.ElevationPrompt.Offers;
        if (expected && offers.Count == 0)
            throw new AssertionException("expected an administrator retry to be offered; none was.");

        if (!expected && offers.Count > 0)
            throw new AssertionException(
                "an administrator retry was offered for: " +
                string.Join("; ", offers.Select(o => $"{o.Operation} ({o.Items.Count} item(s))")));
    }

    // ---- finding elements -------------------------------------------------------------

    /// <summary>
    /// The named element, preferring one belonging to the active tab.
    /// </summary>
    /// <remarks>
    /// A visual-tree walk rather than <c>FindName</c>, because the interesting names are not in
    /// the window's own name scope: <c>PaneLayoutHost</c> builds the pane views in code, and every
    /// open tab has its own <c>FileListView</c>, <c>SearchBox</c> and <c>PathBox</c>. Whichever one
    /// belongs to the tab a command is acting on is the one it means; the first match is a
    /// reasonable answer for the window-level names (<c>FolderTree</c>, <c>GlobalSearchBox</c>)
    /// that exist only once.
    /// </remarks>
    private T? FindNamed<T>(string name) where T : FrameworkElement
    {
        var active = session.Tab;
        T? fallback = null;

        foreach (var element in Descendants(session.Window).OfType<T>())
        {
            if (!element.Name.Equals(name, StringComparison.Ordinal)) continue;
            if (ReferenceEquals(element.DataContext, active)) return element;

            // A per-tab element may carry a DataContext of its own — the preview pane binds to the
            // tab's preview view model rather than to the tab — so ask which tab view it lives in
            // before falling back to whichever copy came first.
            if (ReferenceEquals(OwningTab(element), active)) return element;
            fallback ??= element;
        }

        return fallback;
    }

    private static DirectoryTabViewModel? OwningTab(DependencyObject element)
    {
        for (var d = element; d is not null; d = VisualTreeHelper.GetParent(d))
            if (d is FrameworkElement { DataContext: DirectoryTabViewModel tab }) return tab;
        return null;
    }

    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        var count = VisualTreeHelper.GetChildrenCount(root);
        for (var i = 0; i < count; i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            yield return child;

            foreach (var descendant in Descendants(child)) yield return descendant;
        }
    }

    // ---- odds and ends ----------------------------------------------------------------

    /// <summary>Runs something on the UI thread, then lets the app catch up.</summary>
    private void Invoke(Action action)
    {
        session.Dispatcher.Invoke(action);
        session.Settle();
    }

    /// <summary>
    /// Starts an async view-model call on the UI thread and waits for it there.
    /// </summary>
    /// <remarks>
    /// The task is started inside <c>Dispatcher.Invoke</c> so its continuations capture the UI
    /// thread's context, and waited for by pumping rather than blocking — blocking this thread is
    /// blocking the dispatcher, which is a deadlock, since that is where the continuations want to
    /// run.
    /// </remarks>
    private void Await(Func<Task> start)
    {
        var task = session.Dispatcher.Invoke(start);
        Pump(task);
        session.Settle();
    }

    private T Await<T>(Func<Task<T>> start)
    {
        var task = session.Dispatcher.Invoke(start);
        Pump(task);
        session.Settle();

        return task.Result;
    }

    private void Pump(Task task)
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();

        while (!task.IsCompleted && clock.ElapsedMilliseconds < options.BusyTimeoutMs)
            session.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.Background);

        if (!task.IsCompleted)
            throw new TimeoutException($"The operation did not finish within {options.BusyTimeoutMs} ms.");

        if (task.IsFaulted && task.Exception?.InnerException is { } inner)
            throw new InvalidOperationException(inner.Message, inner);
    }

    /// <summary>Splits a line into its first word and everything after it.</summary>
    private static (string Head, string Tail) Split(string line)
    {
        var space = line.IndexOf(' ');
        return space < 0 ? (line, "") : (line[..space], line[(space + 1)..].Trim());
    }

    /// <summary>
    /// Splits on a separator word — <c>move a.txt, b.txt to Documents</c>.
    /// </summary>
    /// <remarks>
    /// The names half is allowed to be empty (<c>move to Documents</c>, meaning the selection), so
    /// the separator is matched with a leading boundary rather than a leading space — otherwise
    /// that form has nothing before the " to " to find.
    /// </remarks>
    private static (string Before, string After) SplitOn(string line, string separator, string verb)
    {
        var padded = " " + line;
        var at = padded.LastIndexOf(" " + separator + " ", StringComparison.OrdinalIgnoreCase);
        if (at < 0)
            throw new FormatException($"{verb} wants '<names> to <folder>', or 'to <folder>' for the selection.");

        return (padded[..at].Trim(), padded[(at + separator.Length + 2)..].Trim());
    }

    /// <summary>The rest of a line as one argument, with surrounding quotes dropped — a name with
    /// a space in it does not need them, but writing them should not put them in the name.</summary>
    private static string Require(string rest, string verb)
    {
        if (rest.Length == 0) throw new FormatException($"{verb} needs an argument.");

        return rest.Length > 1 && rest[0] == '"' && rest[^1] == '"' ? rest[1..^1] : rest;
    }

    private static int Number(string text, string verb) =>
        int.TryParse(text.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value)
            ? value
            : throw new FormatException($"{verb} needs a number, got '{text}'.");

    private static double Fraction(string text, string verb) =>
        double.TryParse(text.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out var value)
            && value is >= 0 and <= 1
            ? value
            : throw new FormatException($"{verb} needs a number from 0 to 1, got '{text}'.");

    private static bool Switch(string text, string verb) => text.Trim().ToLowerInvariant() switch
    {
        "on" or "true" or "yes" => true,
        "off" or "false" or "no" => false,
        var other => throw new FormatException($"{verb} wants 'on' or 'off', got '{other}'."),
    };

    /// <summary>Resolves a bare capture name against the run's output directory.</summary>
    private string Resolve(string path) =>
        Path.IsPathRooted(path) ? path : Path.Combine(options.OutputDir, path);
}
