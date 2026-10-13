using BertBrowser.App.Services.Commands;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Services.CommandBars;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.Harness;

/// <summary>
/// The menu bar's and toolbars' verbs: arranging them, saving and recalling an arrangement, driving
/// the Menus and toolbars page, and asserting on what the window then shows.
/// </summary>
/// <remarks>
/// An arrangement is changed through <see cref="CommandBarService"/>, the one writer the app has for
/// it, so a script's change redraws the window exactly as a person's does. What no script can do is
/// open a menu — a dropdown is a popup, with a window of its own — so a menu is photographed with
/// <c>menu main File</c>, which renders the rows a click would show, and Alt and F10 are not
/// exercised at all: a run posts no input.
/// </remarks>
internal sealed partial class ScriptRunner
{
    private CommandBarService Bars => session.Services.GetRequiredService<CommandBarService>();

    /// <summary>
    /// <c>command-bars reset</c> · <c>toolbar title|row&lt;n&gt; [text] &lt;item…&gt;</c> · <c>menu
    /// &lt;Name&gt; &lt;item…&gt;</c> · <c>save|apply|remove &lt;name&gt;</c> · <c>rename &lt;old&gt; to
    /// &lt;new&gt;</c> · <c>saved</c>. An item is a command id, <c>-</c> for a separator, or
    /// <c>@list</c> (<c>@workspaces</c>).
    /// </summary>
    private void CommandBars(string rest)
    {
        var (verb, tail) = Split(Require(rest, "command-bars"));

        switch (verb.ToLowerInvariant())
        {
            case "reset":
                Invoke(Bars.Reset);
                break;

            case "toolbar":
            {
                var (which, items) = Split(Require(tail, "command-bars toolbar"));
                var target = ToolbarTarget(which);
                var text = items.StartsWith("text ", StringComparison.OrdinalIgnoreCase) || items.Equals("text", StringComparison.OrdinalIgnoreCase);
                if (text) items = items[4..].Trim();
                var parsed = BarItems(items);

                Invoke(() =>
                {
                    var set = Bars.Current;
                    // A row past the end is made, with any between it and the last.
                    while (target.Kind == BarTargetKind.Row && set.Rows.Count <= target.Row) set = CommandBarEdit.AddRow(set);

                    var spec = new ToolbarSpec(parsed, text);
                    Bars.Set(target.Kind == BarTargetKind.TitleBar
                        ? set with { TitleBar = spec }
                        : set with { Rows = [.. set.Rows.Select((r, i) => i == target.Row ? spec : r)] });
                });
                break;
            }

            // Replaces the menu of that name, or adds one at the end of the bar.
            case "menu":
            {
                var (name, items) = Split(Require(tail, "command-bars menu"));
                var menu = BarItem.Menu(name.Replace('_', ' '), [.. BarItems(items)]);

                Invoke(() =>
                {
                    var set = Bars.Current;
                    var at = set.Menus.ToList().FindIndex(m =>
                        string.Equals(AccessKeys.Strip(m.Caption ?? ""), AccessKeys.Strip(menu.Caption!), StringComparison.OrdinalIgnoreCase));
                    Bars.Set(at < 0
                        ? set with { Menus = [.. set.Menus, menu] }
                        : set with { Menus = [.. set.Menus.Select((m, i) => i == at ? menu : m)] });
                });
                break;
            }

            case "save":
                Invoke(() => Bars.SaveAs(Require(tail, "command-bars save")));
                break;

            // Through the window, so the question an unsaved arrangement raises is asked.
            case "apply":
            {
                var name = Require(tail, "command-bars apply");
                if (!session.Dispatcher.Invoke(() => Bars.IsNameTaken(name)))
                    throw new AssertionException($"There is no saved arrangement called '{name}'.");
                Invoke(() => session.Window.ApplyCommandBars(name));
                break;
            }

            case "rename":
            {
                var (from, to) = SplitOn(tail, "to", "command-bars rename");
                if (!session.Dispatcher.Invoke(() => Bars.Rename(from, to)))
                    throw new AssertionException($"'{from}' could not be renamed to '{to}'.");
                break;
            }

            case "remove":
                Invoke(() => Bars.Remove(Require(tail, "command-bars remove")));
                break;

            case "saved":
                output.WriteLine("SAVED-BARS " + string.Join(", ", session.Dispatcher.Invoke(() => Bars.Saved.Select(s => s.Name).ToList())));
                return;

            default:
                throw new FormatException(
                    $"'{verb}' is not a command-bars verb. Try: reset, toolbar, menu, save, apply, rename, remove, saved.");
        }

        session.Settle();
    }

    private static BarTarget ToolbarTarget(string which)
    {
        if (which.Equals("title", StringComparison.OrdinalIgnoreCase)) return BarTarget.TitleBar;
        if (which.StartsWith("row", StringComparison.OrdinalIgnoreCase) && int.TryParse(which[3..], out var n) && n >= 1)
            return BarTarget.ForRow(n - 1);
        throw new FormatException($"'{which}' is not a toolbar. Say title, or row1, row2….");
    }

    private static List<BarItem> BarItems(string text) =>
        [.. text.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select(token => token switch
        {
            "-" => BarItem.Separator,
            _ when token.StartsWith('@') => CommandBarLists.Find(token[1..]) is { } list
                ? BarItem.List(list.Id)
                : throw new FormatException($"'{token[1..]}' is not a list."),
            // Not checked against the catalogue: an id nothing answers to is a case worth scripting.
            _ => BarItem.Command(token),
        })];

    private CommandBarToolbar Toolbar(string which)
    {
        var target = ToolbarTarget(which);
        return session.Dispatcher.Invoke(() =>
            target.Kind == BarTargetKind.TitleBar ? session.Window.TitleBarToolbar
            : target.Row < session.Window.RowToolbars.Count ? session.Window.RowToolbars[target.Row]
            : throw new AssertionException($"There is no toolbar {which}."));
    }

    private static string Token(BarRow row) => row.Kind switch
    {
        BarRowKind.Separator => "-",
        BarRowKind.Menu => "menu:" + row.Caption,
        BarRowKind.List or BarRowKind.Placeholder => "@" + row.Id,
        _ => row.Id ?? "?",
    };

    /// <summary><c>assert-menu-bar File | Edit | View</c> — the headings, all of them, in order.</summary>
    private void AssertMenuBar(string rest)
    {
        var expected = Require(rest, "assert-menu-bar").Split('|', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
        var actual = session.Dispatcher.Invoke(() => session.Window.MainMenuCaptions);

        if (!actual.SequenceEqual(expected, StringComparer.OrdinalIgnoreCase))
            throw new AssertionException($"The menu bar reads {string.Join(" | ", actual)}, not {string.Join(" | ", expected)}.");
    }

    /// <summary>
    /// <c>assert-toolbar title edit.undo edit.redo - tools.compare</c> — what the toolbar is
    /// <em>showing</em>, exactly, in order. Add <c>+2</c> at the end for how many more are behind
    /// its overflow button; without it, none may be.
    /// </summary>
    private void AssertToolbar(string rest)
    {
        var (which, tail) = Split(Require(rest, "assert-toolbar"));
        var expected = tail.Split(' ', StringSplitOptions.RemoveEmptyEntries).ToList();
        var overflow = 0;
        if (expected.Count > 0 && expected[^1].StartsWith('+') && int.TryParse(expected[^1][1..], out overflow))
            expected.RemoveAt(expected.Count - 1);

        var toolbar = Toolbar(which);
        var (shown, hidden) = session.Dispatcher.Invoke(() =>
            (toolbar.Rows.Take(toolbar.VisibleCount).Select(Token).ToList(), toolbar.Rows.Count - toolbar.VisibleCount));

        if (!shown.SequenceEqual(expected, StringComparer.Ordinal) || hidden != overflow)
        {
            throw new AssertionException(
                $"Toolbar {which} shows [{string.Join(' ', shown)}] with {hidden} overflowed, " +
                $"not [{string.Join(' ', expected)}] with {overflow}.");
        }
    }

    /// <summary><c>assert-toolbar-item title edit.undo disabled</c> — one button's state:
    /// enabled, disabled, checked, unchecked, or <c>text</c>/<c>icon</c> for whether it is labelled.</summary>
    private void AssertToolbarItem(string rest)
    {
        var parts = Require(rest, "assert-toolbar-item").Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length != 3)
            throw new FormatException("assert-toolbar-item wants <title|rowN> <command id> <enabled|disabled|checked|unchecked|text|icon>.");

        var toolbar = Toolbar(parts[0]);
        var row = session.Dispatcher.Invoke(() => toolbar.Rows.FirstOrDefault(r => r.Id == parts[1]))
                  ?? throw new AssertionException($"Toolbar {parts[0]} has no {parts[1]}.");

        var ok = parts[2].ToLowerInvariant() switch
        {
            "enabled" => row.IsEnabled,
            "disabled" => !row.IsEnabled,
            "checked" => row.IsChecked,
            "unchecked" => !row.IsChecked,
            "text" => row.ShowText,
            "icon" => !row.ShowText,
            var other => throw new FormatException($"'{other}' is not a state."),
        };
        if (!ok)
        {
            throw new AssertionException(
                $"{parts[1]} on toolbar {parts[0]} is not {parts[2]} (enabled={row.IsEnabled}, checked={row.IsChecked}, " +
                $"text={row.ShowText}, reason={row.Reason ?? "none"}).");
        }
    }

    /// <summary><c>assert-bars default</c>, <c>assert-bars custom</c>, or the name of the saved
    /// arrangement that should be the one in force.</summary>
    private void AssertBars(string rest)
    {
        var expected = Require(rest, "assert-bars");
        var actual = session.Dispatcher.Invoke(() => Bars.IsDefault ? "default" : Bars.CurrentName ?? "custom");

        if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"The menus and toolbars are '{actual}', not '{expected}'.");
    }

    // --- The Menus and toolbars page ---

    /// <summary>
    /// <c>settings bars select menu-bar|title|row&lt;n&gt;</c> · <c>open &lt;name&gt;</c> · <c>up</c> ·
    /// <c>add &lt;item&gt;</c> · <c>add-menu</c> · <c>remove &lt;name&gt;</c> · <c>nudge &lt;name&gt;
    /// up|down</c> · <c>name &lt;text&gt;</c> · <c>filter &lt;text&gt;</c> · <c>rows</c>. The page's own
    /// view model does each one, so what a click reaches is what this reaches.
    /// </summary>
    private void SettingsBars(string rest)
    {
        var (verb, tail) = Split(Require(rest, "settings bars"));

        Invoke(() =>
        {
            var vm = session.Window.OpenSettings
                     ?? throw new AssertionException("settings bars needs the settings page open: 'settings command-bars' first.");

            BarItemRowViewModel Row(string name) =>
                vm.BarRows.FirstOrDefault(r => r.Name.Equals(name, StringComparison.OrdinalIgnoreCase))
                ?? throw new AssertionException($"There is no '{name}' here. Rows: {string.Join(", ", vm.BarRows.Select(r => r.IsSeparator ? "-" : r.Name))}");

            switch (verb.ToLowerInvariant())
            {
                case "select":
                {
                    var target = tail.Equals("menu-bar", StringComparison.OrdinalIgnoreCase) ? BarTarget.MenuBar : ToolbarTarget(tail);
                    vm.SelectedBar = vm.BarTargets.FirstOrDefault(t => t.Target == target)
                                     ?? throw new AssertionException($"There is no bar '{tail}'.");
                    break;
                }
                case "open":
                    vm.SelectedBarRow = Row(tail);
                    if (!vm.OpenBarRowCommand.CanExecute(null)) throw new AssertionException($"'{tail}' is not a menu.");
                    vm.OpenBarRowCommand.Execute(null);
                    break;
                case "up":
                    vm.BarUpCommand.Execute(null);
                    break;
                case "pick":
                    vm.SelectedBarRow = Row(tail);
                    break;
                case "add":
                {
                    var item = BarItems(tail).Single();
                    if (item.IsSeparator) vm.AddBarSeparatorCommand.Execute(null);
                    else vm.InsertBarChoice(vm.BarChoices.FirstOrDefault(c => c.Item is { } i && i.Kind == item.Kind && i.Id == item.Id)
                                            ?? throw new AssertionException($"The picker does not offer '{tail}'."),
                        vm.SelectedBarRow is { } selected ? vm.BarRows.IndexOf(selected) + 1 : vm.BarRows.Count);
                    break;
                }
                case "add-menu":
                    vm.AddBarSubmenuCommand.Execute(null);
                    break;
                case "remove":
                    vm.RemoveBarRowCommand.Execute(Row(tail));
                    break;
                case "nudge":
                {
                    var split = tail.LastIndexOf(' ');
                    var direction = split < 0 ? "" : tail[(split + 1)..];
                    if (direction is not ("up" or "down")) throw new FormatException("settings bars nudge wants <name> up|down.");
                    vm.SelectedBarRow = Row(tail[..split].Trim());
                    vm.NudgeSelectedBarRow(direction == "up" ? -1 : 1);
                    break;
                }
                case "name":
                    vm.BarCaption = tail;
                    vm.CommitBarCaptionCommand.Execute(null);
                    break;
                case "filter":
                    vm.BarFilter = tail;
                    break;
                case "unplaced":
                    vm.BarUnplacedOnly = tail is "on";
                    break;
                case "rows":
                    output.WriteLine($"BAR-ROWS {vm.BarLocation}: " +
                                     string.Join(" | ", vm.BarRows.Select(r => r.IsSeparator ? "-" : r.Name)));
                    break;
                case "choices":
                    output.WriteLine("BAR-CHOICES " + string.Join(" | ",
                        vm.BarChoices.Select(c => c.IsHeading ? $"# {c.Name}" : c.Name)));
                    break;
                default:
                    throw new FormatException(
                        $"'{verb}' is not a settings bars verb. Try: select, open, up, pick, add, add-menu, remove, nudge, name, filter, unplaced, rows, choices.");
            }
        });
        session.Settle();
    }

    /// <summary><c>assert-bar-rows Undo | Redo | - | Settings</c> — the middle list of the Menus
    /// and toolbars page, exactly.</summary>
    private void AssertBarRows(string rest)
    {
        var expected = Require(rest, "assert-bar-rows").Split('|', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
        var actual = session.Dispatcher.Invoke(() =>
            session.Window.OpenSettings?.BarRows.Select(r => r.IsSeparator ? "-" : r.Name).ToList()
            ?? throw new AssertionException("assert-bar-rows needs the settings page open."));

        if (!actual.SequenceEqual(expected, StringComparer.OrdinalIgnoreCase))
            throw new AssertionException($"The page lists {string.Join(" | ", actual)}, not {string.Join(" | ", expected)}.");
    }

    /// <summary><c>assert-bar-choice Next tab</c> / <c>assert-no-bar-choice …</c> — whether the
    /// picker is offering something, after its filter.</summary>
    private void AssertBarChoice(string rest, bool expected)
    {
        var name = Require(rest, expected ? "assert-bar-choice" : "assert-no-bar-choice");
        var offered = session.Dispatcher.Invoke(() =>
            (session.Window.OpenSettings ?? throw new AssertionException("This needs the settings page open."))
            .BarChoices.Any(c => !c.IsHeading && c.Name.Equals(name, StringComparison.OrdinalIgnoreCase)));

        if (offered != expected)
            throw new AssertionException(expected ? $"The picker does not offer '{name}'." : $"The picker offers '{name}'.");
    }
}
