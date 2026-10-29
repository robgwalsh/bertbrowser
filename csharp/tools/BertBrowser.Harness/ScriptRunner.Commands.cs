using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Services.Commands;
using BertBrowser.Core.Services.Commands;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.Harness;

/// <summary>
/// The command layer's verbs: pressing a chord, running a command by id, rebinding one.
/// </summary>
/// <remarks>
/// A run posts no input, so <c>key</c> is not a key press. It asks the same
/// <see cref="KeymapRules.Dispatch"/> the window's key handler asks — with the focus posed, since
/// nothing here can put real focus anywhere — and runs what it names through the same registry.
/// That is every decision between a key and a command; what it cannot show is WPF delivering the
/// key in the first place, which only a person at the keyboard can.
/// </remarks>
internal sealed partial class ScriptRunner
{
    /// <summary>Commands a run must not carry out, whoever asks: the clipboard is the user's, and
    /// there is one per session.</summary>
    private static readonly HashSet<string> ClipboardCommands =
        ["edit.cut", "edit.copy", "edit.paste", "edit.copy-path", "edit.copy-name"];

    private KeymapService Keymap => session.Services.GetRequiredService<KeymapService>();

    /// <summary><c>key Ctrl+T</c>, <c>key Del in list</c>, <c>key Backspace in text</c>.</summary>
    private void Key(string rest)
    {
        var (chord, focus) = ChordAndFocus(Require(rest, "key"), "key");
        var command = Dispatch(chord, focus);

        if (command is null)
        {
            output.WriteLine($"KEY {chord} -> nothing");
            return;
        }

        output.WriteLine($"KEY {chord} -> {command}");
        Run(command);
    }

    /// <summary><c>assert-key Ctrl+C text none</c>, <c>assert-key F2 list file.rename</c>.</summary>
    private void AssertKey(string rest)
    {
        var parts = Require(rest, "assert-key").Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length != 3)
            throw new FormatException("assert-key wants <chord> <list|text|readonly|other> <command id|none>.");

        var chord = KeyChord.Parse(parts[0]);
        var actual = Dispatch(chord, Focus(parts[1])) ?? "none";

        if (!actual.Equals(parts[2], StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"{chord} with focus in '{parts[1]}' runs {actual}, not {parts[2]}.");
    }

    /// <summary><c>run tab.new</c> — what choosing the command in the palette does.</summary>
    private void Run(string id)
    {
        id = Require(id, "run");
        var registry = session.Window.Commands;
        var handler = session.Dispatcher.Invoke(() => registry.Find(id))
            ?? throw new FormatException($"'{id}' is not a command.");

        if (session.Dispatcher.Invoke(() => registry.Unavailable(id)) is { } reason)
        {
            output.WriteLine($"UNAVAILABLE {id} — {reason}");
            return;
        }

        // A dialog could never be dismissed and the clipboard is not this run's to write, so both
        // are reported. Everything up to here — the binding, the focus rule, the availability
        // rule — has still been exercised for real.
        if (handler.NeedsAPerson || ClipboardCommands.Contains(id))
        {
            output.WriteLine($"WOULD-RUN {id}");
            return;
        }

        Invoke(() => registry.TryExecute(id));
    }

    /// <summary><c>assert-command file.rename available</c>,
    /// <c>assert-command edit.cut unavailable inside an archive</c>.</summary>
    private void AssertCommand(string rest)
    {
        var (id, tail) = Split(Require(rest, "assert-command"));
        var (expected, text) = Split(tail);
        var wantAvailable = expected.ToLowerInvariant() switch
        {
            "available" => true,
            "unavailable" => false,
            _ => throw new FormatException("assert-command wants <id> available|unavailable [reason]."),
        };

        var reason = session.Dispatcher.Invoke(() => session.Window.Commands.Unavailable(id));

        if (wantAvailable && reason is not null)
            throw new AssertionException($"{id} is unavailable: {reason}");
        if (!wantAvailable && reason is null)
            throw new AssertionException($"{id} is available.");
        if (!wantAvailable && text.Length > 0 && !reason!.Contains(text, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"{id} is unavailable because \"{reason}\", which does not mention '{text}'.");
    }

    /// <summary><c>bind view.flat Ctrl+K</c> gives a command exactly those chords;
    /// <c>bind view.flat none</c> takes its shortcut away.</summary>
    private void Bind(string rest)
    {
        var (id, tail) = Split(Require(rest, "bind"));
        var chords = tail.Equals("none", StringComparison.OrdinalIgnoreCase)
            ? []
            : tail.Split(' ', StringSplitOptions.RemoveEmptyEntries).Select(KeyChord.Parse).ToList();

        if (tail.Length == 0)
            throw new FormatException("bind wants <command id> <chord>… or <command id> none.");

        var keymap = Keymap;
        var context = keymap.Current.ContextOf(id)
            ?? throw new FormatException($"'{id}' is not something a shortcut can be given to.");
        foreach (var chord in chords)
        {
            if (KeymapRules.Refuse(chord, context) is { } refusal)
                throw new InvalidOperationException($"{chord} cannot be bound to {id}: {refusal}");
        }

        Invoke(() => keymap.SetChords(id, chords));
    }

    /// <summary><c>unbind view.flat</c> puts one command back to its shipped shortcut;
    /// bare <c>unbind</c> does it for all of them.</summary>
    private void Unbind(string rest)
    {
        var keymap = Keymap;
        if (rest.Length == 0) Invoke(keymap.ResetAll);
        else Invoke(() => keymap.Reset(rest));
    }

    /// <summary><c>assert-gesture view.flat Ctrl+B</c> — what a menu or tooltip would print.</summary>
    private void AssertGesture(string rest)
    {
        var (id, expected) = Split(Require(rest, "assert-gesture"));
        var actual = Keymap.GestureText(id);
        if (expected.Equals("none", StringComparison.OrdinalIgnoreCase)) expected = "";

        if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"{id} shows '{actual}', not '{expected}'.");
    }

    /// <summary>Every visible line of the last <c>menu</c> with the shortcut printed beside it.</summary>
    private List<(string Header, string Gesture)> _lastMenuGestures = [];

    private static IEnumerable<(string, string)> MenuGestures(IEnumerable<FrameworkElement> items)
    {
        foreach (var item in items)
        {
            if (item is not MenuItem { Visibility: Visibility.Visible } menuItem) continue;
            if (menuItem.Header is string header) yield return (header, menuItem.InputGestureText ?? "");
            foreach (var nested in MenuGestures(menuItem.Items.OfType<FrameworkElement>())) yield return nested;
        }
    }

    /// <summary><c>assert-menu-gesture Rename F2</c> — what the last <c>menu</c> printed beside an
    /// item. <c>none</c> for an item with no shortcut.</summary>
    private void AssertMenuGesture(string rest)
    {
        rest = Require(rest, "assert-menu-gesture");
        var split = rest.LastIndexOf(' ');
        if (split < 0) throw new FormatException("assert-menu-gesture wants <item text> <chord|none>.");

        var (text, expected) = (rest[..split].Trim(), rest[(split + 1)..].Trim());
        if (expected.Equals("none", StringComparison.OrdinalIgnoreCase)) expected = "";

        var match = _lastMenuGestures.FirstOrDefault(m => m.Header.Contains(text, StringComparison.OrdinalIgnoreCase));
        if (match.Header is null)
            throw new AssertionException($"no item of the last menu contains '{text}'.");
        if (!match.Gesture.Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"'{match.Header}' shows '{match.Gesture}', not '{expected}'.");
    }

    /// <summary><c>assert-tooltip PreviewPaneToggle Ctrl+P</c> — a named element's tooltip text.</summary>
    private void AssertToolTip(string rest)
    {
        var (name, text) = Split(Require(rest, "assert-tooltip"));

        var actual = session.Dispatcher.Invoke(() =>
            (FindNamed<FrameworkElement>(name)
             ?? throw new AssertionException($"there is no element named {name}.")).ToolTip as string ?? "");

        if (!actual.Contains(text, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"{name}'s tooltip is \"{actual}\", which does not contain '{text}'.");
    }

    // ---- the Keyboard settings page ------------------------------------------------------

    private BertBrowser.App.ViewModels.SettingsViewModel KeyboardPage =>
        session.Window.OpenSettings ?? throw new InvalidOperationException("The settings page is not open.");

    /// <summary>
    /// <c>settings key select &lt;command id&gt;</c> · <c>filter &lt;text&gt;</c> · <c>change</c> ·
    /// <c>add</c> · <c>press &lt;chord&gt;</c> · <c>reassign</c> · <c>remove</c> · <c>reset</c> ·
    /// <c>reset-all</c> · <c>find</c> · <c>cancel</c>. <c>press</c> is the recorder being offered
    /// a chord — the step after the key event has been turned into one.
    /// </summary>
    private void SettingsKey(string rest)
    {
        var (verb, tail) = Split(rest);
        var page = KeyboardPage;

        Invoke(() =>
        {
            switch (verb.ToLowerInvariant())
            {
                case "select": page.SelectKeyCommand(Require(tail, "settings key select")); break;
                case "filter": page.KeyFilter = tail; break;
                case "change": Execute(page.ChangeKeyCommand); break;
                case "add": Execute(page.AddKeyCommand); break;
                case "remove": Execute(page.RemoveKeysCommand); break;
                case "reset": Execute(page.ResetKeyCommand); break;
                case "reset-all": Execute(page.ResetAllKeysCommand); break;
                case "reassign": Execute(page.ReassignKeyCommand); break;
                case "find": Execute(page.FindKeyCommand); break;
                case "cancel": page.CancelKeyCapture(); break;
                case "press":
                    if (!page.IsCapturingKey)
                        throw new AssertionException("The Keyboard page is not listening for a key.");
                    page.OfferChord(KeyChord.Parse(Require(tail, "settings key press")));
                    break;
                default:
                    throw new FormatException(
                        $"settings key does not know '{verb}'. Try: select, filter, change, add, press, " +
                        "reassign, remove, reset, reset-all, find, cancel.");
            }
        });

        static void Execute(System.Windows.Input.ICommand command)
        {
            if (!command.CanExecute(null))
                throw new AssertionException("That button is disabled on the Keyboard page right now.");
            command.Execute(null);
        }
    }

    private void AssertKeyRecorder(string rest)
    {
        var text = Require(rest, "assert-key-recorder");
        var actual = session.Dispatcher.Invoke(() => KeyboardPage.RecorderText);

        if (!actual.Contains(text, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"the recorder says \"{actual}\", which does not contain '{text}'.");
    }

    /// <summary><c>assert-key-row New tab Ctrl+T</c> — a row of the Keyboard page's list and the
    /// shortcuts beside it, comma-separated; <c>none</c> for a command with no shortcut, and
    /// <c>absent</c> for a command the filter has taken out of the list.</summary>
    private void AssertKeyRow(string rest)
    {
        rest = Require(rest, "assert-key-row");
        var split = rest.LastIndexOf(' ');
        if (split < 0) throw new FormatException("assert-key-row wants <command name> <chords|none|absent>.");

        var (name, expected) = (rest[..split].Trim(), rest[(split + 1)..].Trim());
        var row = session.Dispatcher.Invoke(() => KeyboardPage.KeyRows
            .FirstOrDefault(r => !r.IsHeading && r.Name.Equals(name, StringComparison.OrdinalIgnoreCase)));

        if (expected.Equals("absent", StringComparison.OrdinalIgnoreCase))
        {
            if (row is not null) throw new AssertionException($"'{name}' is listed, and should not be.");
            return;
        }

        if (row is null) throw new AssertionException($"the Keyboard page lists no command called '{name}'.");

        var actual = row.Chords.Count == 0 ? "none" : string.Join(",", row.Chords);
        if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"'{name}' shows {actual}, not {expected}.");
    }

    private string? Dispatch(KeyChord chord, FocusKind focus) => session.Dispatcher.Invoke(() =>
    {
        // The window's own answer for everything but where focus is, which is posed.
        var state = session.Window.FocusNow() with { Focus = focus };
        return KeymapRules.Dispatch(Keymap.Current, chord, state);
    });

    private static (KeyChord Chord, FocusKind Focus) ChordAndFocus(string rest, string verb)
    {
        var at = rest.IndexOf(" in ", StringComparison.OrdinalIgnoreCase);
        return at < 0
            ? (KeyChord.Parse(rest), FocusKind.FileList)
            : (KeyChord.Parse(rest[..at]), Focus(rest[(at + 4)..]));
    }

    private static FocusKind Focus(string word) => word.Trim().ToLowerInvariant() switch
    {
        "list" => FocusKind.FileList,
        "text" => FocusKind.TextEditable,
        "readonly" => FocusKind.TextReadOnly,
        "other" or "tree" => FocusKind.Other,
        _ => throw new FormatException($"Focus is one of list, text, readonly or other, not '{word}'."),
    };
}
