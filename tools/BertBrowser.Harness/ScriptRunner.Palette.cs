using BertBrowser.App.ViewModels;

namespace BertBrowser.Harness;

/// <summary>
/// The command palette's verbs. They drive its view model the way the keyboard drives it — typing
/// sets the box, <c>down</c> and <c>up</c> move the selection, <c>pick</c> is Enter — so a script
/// goes through everything between the box and the command, and <c>shot</c> photographs the
/// overlay in the window, where it really is.
/// </summary>
internal sealed partial class ScriptRunner
{
    private CommandPaletteViewModel OpenPalette =>
        session.Window.Palette ?? throw new InvalidOperationException("The palette is not open.");

    /// <summary>
    /// <c>palette</c> / <c>palette open [seed]</c> · <c>palette type &lt;text&gt;</c> ·
    /// <c>palette down|up [n]</c> · <c>palette pick</c> · <c>palette tab</c> · <c>palette pin</c> ·
    /// <c>palette shortcut</c> ·
    /// <c>palette rows</c> · <c>palette close</c>.
    /// </summary>
    private void Palette(string rest)
    {
        var (verb, tail) = Split(rest);

        switch (verb.ToLowerInvariant())
        {
            case "" or "open":
                Invoke(() =>
                {
                    if (session.Window.IsPaletteOpen) session.Window.ClosePalette();
                    session.Window.ShowPalette(tail);
                });
                break;

            case "close":
                Invoke(() => session.Window.ClosePalette());
                break;

            // Empty text is a real thing to type: it is how a script gets back to the unasked list.
            case "type":
                Invoke(() => OpenPalette.Query = tail);
                break;

            case "clear":
                Invoke(() => OpenPalette.Query = "");
                break;

            case "down":
                Invoke(() => OpenPalette.Move(tail.Length == 0 ? 1 : Number(tail, "palette down")));
                break;

            case "up":
                Invoke(() => OpenPalette.Move(tail.Length == 0 ? -1 : -Number(tail, "palette up")));
                break;

            case "tab":
                Invoke(() => OpenPalette.Complete());
                break;

            case "shortcut":
                Invoke(() => OpenPalette.ChangeShortcut());
                break;

            case "pin":
                Invoke(() => OpenPalette.TogglePin());
                break;

            case "pick":
                PickFromPalette();
                break;

            case "rows":
                output.WriteLine("PALETTE " + string.Join(" | ", session.Dispatcher.Invoke(PaletteLines)));
                break;

            default:
                throw new FormatException(
                    $"palette does not know '{verb}'. Try: open, type, clear, down, up, tab, pin, shortcut, pick, rows, close.");
        }
    }

    /// <summary>Enter. A command that would put a window up or write the clipboard is reported
    /// rather than run, as <c>run</c> reports it; the palette closes either way.</summary>
    private void PickFromPalette()
    {
        var (id, needsAPerson) = session.Dispatcher.Invoke(() =>
        {
            var row = OpenPalette.SelectedRow;
            var entry = row?.Row.Entry;
            var person = entry is { IsAvailable: true } &&
                         (session.Window.Commands.Find(entry.Id) is { NeedsAPerson: true } ||
                          ClipboardCommands.Contains(entry.Id));
            return (entry?.Id ?? row?.Text ?? "nothing", person);
        });

        if (needsAPerson)
        {
            output.WriteLine($"WOULD-RUN {id}");
            Invoke(() => session.Window.ClosePalette());
            return;
        }

        output.WriteLine($"PICK {id}");
        Invoke(() => OpenPalette.Activate());
    }

    /// <summary>The list as text: headings as <c># Heading</c>, categories as their token, an
    /// unavailable entry in brackets, and the selected row marked with <c>&gt;</c>.</summary>
    private List<string> PaletteLines()
    {
        var palette = OpenPalette;
        return [.. palette.Rows.Select(row =>
        {
            var text = row.IsHeading ? $"# {row.Text}"
                : row.IsCategory ? row.Row.Token
                : row.IsAvailable ? row.Text
                : $"({row.Text})";
            return ReferenceEquals(row, palette.SelectedRow) ? "> " + text : text;
        })];
    }

    private List<string> PaletteNames() =>
        session.Dispatcher.Invoke(() => OpenPalette.Rows
            .Select(row => row.IsHeading ? $"# {row.Text}" : row.IsCategory ? row.Row.Token : row.Text)
            .ToList());

    private void AssertPaletteRow(string rest, bool expected)
    {
        var text = Require(rest, expected ? "assert-palette-row" : "assert-no-palette-row");
        var rows = PaletteNames();
        var actual = rows.Any(r => r.Contains(text, StringComparison.OrdinalIgnoreCase));

        if (actual != expected)
            throw new AssertionException(expected
                ? $"no palette row contains '{text}'. Rows: {string.Join(" | ", rows)}"
                : $"a palette row contains '{text}', and none should.");
    }

    /// <summary><c>assert-palette-order New tab | Close tab</c> — these rows, in this order, not
    /// necessarily adjacent.</summary>
    private void AssertPaletteOrder(string rest)
    {
        var wanted = Require(rest, "assert-palette-order")
            .Split('|', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        var rows = PaletteNames();

        var at = 0;
        foreach (var want in wanted)
        {
            var found = rows.FindIndex(at, r => r.Contains(want, StringComparison.OrdinalIgnoreCase));
            if (found < 0)
                throw new AssertionException(
                    $"'{want}' does not come after the rows before it. Rows: {string.Join(" | ", rows)}");
            at = found + 1;
        }
    }

    private void AssertPaletteSelected(string rest)
    {
        var text = Require(rest, "assert-palette-selected");
        var selected = session.Dispatcher.Invoke(() => OpenPalette.SelectedRow?.Text ?? "");

        if (!selected.Contains(text, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"the selected row is '{selected}', not '{text}'.");
    }

    private void AssertPaletteHint(string rest)
    {
        var text = Require(rest, "assert-palette-hint");
        var hint = session.Dispatcher.Invoke(() => OpenPalette.Hint);

        if (!hint.Contains(text, StringComparison.OrdinalIgnoreCase))
            throw new AssertionException($"the palette's hint is \"{hint}\", which does not contain '{text}'.");
    }

    private void AssertPaletteOpen(bool expected)
    {
        if (session.Dispatcher.Invoke(() => session.Window.IsPaletteOpen) != expected)
            throw new AssertionException(expected ? "the palette is not open." : "the palette is open.");
    }
}
