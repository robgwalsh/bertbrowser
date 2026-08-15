using System.Globalization;
using System.Text;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Data;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Delete;
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

            // navigation
            case "go": Go(rest); break;
            case "up": Invoke(() => session.Tab.UpCommand.Execute(null)); break;
            case "back": Invoke(() => session.Tab.BackCommand.Execute(null)); break;
            case "forward": Invoke(() => session.Tab.ForwardCommand.Execute(null)); break;
            case "refresh": Invoke(() => session.Tab.RefreshCommand.Execute(null)); break;
            case "enter": Enter(rest); break;

            // selection
            case "select": Select(rest); break;
            case "select-all": SelectAll(); break;
            case "deselect": SetSelection([]); break;

            // tabs and panes
            case "newtab": NewTab(rest); break;
            case "closetab": Invoke(() => session.Shell.ActivePane.CloseTab(session.Tab)); break;
            case "tab": ActivateTab(rest); break;
            case "split": Split(rest, out _); break;
            case "closepane": Invoke(() => session.Shell.ClosePane(session.Shell.ActivePane)); break;
            case "pane": ActivatePane(rest); break;

            // searching
            case "search": Search(rest, global: false); break;
            case "gsearch": Search(rest, global: true); break;
            case "clear-search": Invoke(() => session.Tab.ClearSearchCommand.Execute(null)); break;

            // acting on the selection
            case "rename": Rename(rest); break;
            case "delete": Delete(rest, DeleteMode.Recycle); break;
            case "delete-permanent": Delete(rest, DeleteMode.Permanent); break;
            case "move": Transfer(rest, TransferVerb.Move); break;
            case "copy": Transfer(rest, TransferVerb.Copy); break;
            case "undo": Undo(); break;

            // browse settings
            case "hidden": Hidden(rest); break;
            case "thumbnails": Thumbnails(rest); break;
            case "sort": Sort(rest); break;
            case "theme": Theme(rest); break;

            // capturing and reading back
            case "shot": Shot(rest); break;
            case "dialog": Dialog(rest); break;
            case "state": output.WriteLine("STATE " + State()); break;
            case "rows": output.WriteLine("ROWS " + string.Join(", ", RowNames())); break;

            // assertions
            case "assert-path": AssertPath(rest); break;
            case "assert-status": AssertStatus(rest); break;
            case "assert-count": AssertCount(rest); break;
            case "assert-row": AssertRow(rest, expected: true); break;
            case "assert-no-row": AssertRow(rest, expected: false); break;
            case "assert-selected": AssertSelected(rest); break;
            case "assert-tabs": AssertTabs(rest); break;
            case "assert-panes": AssertPanes(rest); break;
            case "assert-flattened": AssertFlattened(expected: true); break;
            case "assert-not-flattened": AssertFlattened(expected: false); break;
            case "assert-can-undo": AssertCanUndo(expected: true); break;
            case "assert-cannot-undo": AssertCanUndo(expected: false); break;
            case "assert-exists": AssertOnDisk(rest, expected: true); break;
            case "assert-missing": AssertOnDisk(rest, expected: false); break;
            case "assert-visible": AssertVisibility(rest, expected: true); break;
            case "assert-hidden": AssertVisibility(rest, expected: false); break;
            case "assert-not-launched": AssertNotLaunched(); break;

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
        if (!Directory.Exists(path)) throw new AssertionException($"There is no folder at '{path}'.");

        Await(() => session.Tab.NavigateToAsync(path));
    }

    /// <summary>Opens the named row, exactly as double-clicking or pressing Enter on it does.</summary>
    /// <remarks>
    /// A file is refused rather than opened: the launcher this run is given starts nothing, so the
    /// only outcome would be a status-bar message, and saying so up front beats a script that looks
    /// like it did something.
    /// </remarks>
    private void Enter(string rest)
    {
        var name = Require(rest, "enter");
        var row = Row(name);

        if (!row.IsDirectory)
            throw new InvalidOperationException(
                $"'{name}' is a file, and this run starts no programs. Use 'go' for folders, or " +
                "assert on the status bar if you meant to check the refusal.");

        Invoke(() => session.Tab.Open(row, elevated: false));
        session.Settle();
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

        if (global && !options.Index)
            throw new InvalidOperationException(
                "Whole-PC search reads the MFT index, which this run did not build. Start the " +
                "harness with --index (it needs elevation and takes minutes), or use 'search', " +
                "which walks the current folder.");

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
        var sources = Selection()
            .Select(i => new RenameSource(_sandbox.RequireInside(i.FullPath, "rename"), i.IsDirectory))
            .ToList();

        var plan = session.Dispatcher.Invoke(() => session.Shell.PlanRename(sources, pattern));

        if (plan.Rejected is { Count: > 0 } rejected)
            throw new AssertionException(
                $"The rename was refused: {string.Join("; ", rejected.Select(r => r.Message))}");

        if (!plan.HasWork) throw new AssertionException($"Renaming to '{pattern}' would change nothing.");

        var outcome = Await(() => session.Shell.RenameAsync(plan));

        if (outcome.Failed is { Count: > 0 } failures)
            throw new AssertionException(string.Join("; ", failures.Select(f => f.Message)));
    }

    /// <summary>Deletes the selection, as the confirmation's Delete button does.</summary>
    private void Delete(string rest, DeleteMode mode)
    {
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

        if (outcome.Failed is { Count: > 0 } failures)
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

    private void Undo()
    {
        if (!session.Dispatcher.Invoke(() => session.Shell.CanUndo))
            throw new AssertionException("There is nothing to undo.");

        Invoke(() => session.Shell.UndoCommand.Execute(null));
        session.Settle();
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

    private void Sort(string rest)
    {
        var column = Require(rest, "sort").ToLowerInvariant() switch
        {
            "name" => SortColumn.Name,
            "size" => SortColumn.Size,
            "modified" or "date" => SortColumn.Modified,
            "type" => SortColumn.Type,
            var other => throw new FormatException(
                $"sort wants name, size, modified or type, got '{other}'."),
        };

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

    private Window Build(string kind) => kind switch
    {
        "rename" => RenameDialog.Create(
            Selection().Select(i => new RenameSource(i.FullPath, i.IsDirectory)).ToList(),
            session.Shell.PlanRename),

        "delete" => DeleteDialog.Create(
            DeletePlanFor(DeleteMode.Recycle), session.Shell.SurveyDelete),

        "delete-permanent" => DeleteDialog.Create(
            DeletePlanFor(DeleteMode.Permanent), session.Shell.SurveyDelete),

        "message" => MessageDialog.Create(
            "The harness built this dialog to photograph it. Nothing went wrong.",
            "Message", MessageDialogKind.Information),

        "warning" => MessageDialog.Create(
            "The harness built this dialog to photograph it. Nothing went wrong.",
            "Warning", MessageDialogKind.Warning, showCancel: true),

        "properties" => new PropertiesDialog(new PropertiesViewModel(
            Selection().Select(i => new PropertiesTarget(i.FullPath, i.IsDirectory)).ToList(),
            session.Services.GetRequiredService<DirSizeRepository>())),

        "settings" => new SettingsWindow(new SettingsViewModel(
            session.Services.GetRequiredService<AppSettings>(),
            session.Services.GetRequiredService<IThemeService>())),

        "theme-editor" => new ThemeEditorWindow(new AppearanceViewModel(
            session.Services.GetRequiredService<IThemeService>())),

        _ => throw new FormatException(
            $"'{kind}' is not a dialog. Try: rename, delete, delete-permanent, message, warning, " +
            "properties, settings, theme-editor."),
    };

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
            ("search", Quote(tab.ActiveSearchText)),
            ("globalSearch", Bool(tab.IsGlobalSearch)),
            ("tabs", Text(shell.ActivePane.Tabs.Count)),
            ("panes", Text(shell.AllPanes.Count())),
            ("hidden", Bool(shell.ShowHiddenItems)),
            ("thumbnails", tab.FileList.ThumbnailScale.ToString("0.##", CultureInfo.InvariantCulture)),
            ("canUndo", Bool(shell.CanUndo)),
            ("undo", Quote(shell.UndoDescription)),
            ("foregroundCorrections", Text(session.ForegroundCorrections)),
            ("selection", Quote(tab.SelectionSummary)),
            ("status", Quote(tab.StatusText)),
        };

        return "{" + string.Join(",", fields.Select(f => $"{Quote(f.Name)}:{f.Value}")) + "}";
    });

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

    private void AssertStatus(string rest)
    {
        var expected = Require(rest, "assert-status");
        var actual = session.Dispatcher.Invoke(() => session.Tab.StatusText);

        if (!actual.Contains(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"expected the status to contain '{expected}', got '{actual}'.");
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
            fallback ??= element;
        }

        return fallback;
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
