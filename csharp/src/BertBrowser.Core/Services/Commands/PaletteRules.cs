namespace BertBrowser.Core.Services.Commands;

/// <summary>
/// One thing the palette can offer: a catalogue command, or something made from data — a saved
/// workspace to switch to, a bookmark to open, a theme to wear.
/// </summary>
/// <param name="Id">The command's id, or for a data entry one of its own (<c>workspace:Work</c>).
/// What pins and recents remember.</param>
/// <param name="Category">The group it is listed under and narrowed by.</param>
/// <param name="Gesture">Its shortcut as shown, or empty.</param>
/// <param name="Unavailable">Why it cannot run right now, or null when it can.</param>
/// <param name="Detail">A second line of sorts — a path, a scope — shown beside the name.</param>
public sealed record PaletteEntry(
    string Id,
    string Name,
    string Category,
    string? Icon = null,
    string Gesture = "",
    IReadOnlyList<string>? Aliases = null,
    CommandProminence Prominence = CommandProminence.Standard,
    string? Unavailable = null,
    string Detail = "")
{
    public bool IsAvailable => Unavailable is null;
}

public enum PaletteRowKind
{
    /// <summary>A label over the rows beneath it. Not choosable.</summary>
    Heading,

    /// <summary>Something to run.</summary>
    Entry,

    /// <summary>A category to narrow to. Choosing it puts its token in the box.</summary>
    Category,
}

/// <summary>One line of the palette's list.</summary>
/// <param name="Text">The heading, the entry's name, or the category's.</param>
/// <param name="Highlights">Where in <see cref="Text"/> the typed words matched.</param>
/// <param name="Token">For a category row, what choosing it types: <c>view:</c>.</param>
/// <param name="Count">For a category row, how many entries it holds.</param>
public sealed record PaletteRow(
    PaletteRowKind Kind,
    string Text,
    PaletteEntry? Entry = null,
    IReadOnlyList<MatchRange>? Highlights = null,
    string Token = "",
    int Count = 0);

/// <summary>What the palette knows about the person using it, beyond what they typed.</summary>
/// <param name="Pins">Entries they pinned, in the order they pinned them. Persisted.</param>
/// <param name="Recents">Entries they ran this session, most recent first. Never persisted.</param>
/// <param name="Suggested">What to offer unasked for where the window is right now, best first —
/// see <see cref="PaletteSuggestions"/>.</param>
public sealed record PaletteState(
    IReadOnlyList<string> Pins,
    IReadOnlyList<string> Recents,
    IReadOnlyList<string> Suggested);

/// <summary>
/// What the palette lists for what has been typed — and, as much to the point, what it leaves out.
/// </summary>
/// <remarks>
/// <para>
/// <b>An empty box never lists everything.</b> A couple of hundred commands in catalogue order is
/// a wall, and the useful dozen are somewhere in it. So with nothing typed the list is what this
/// person pinned, what they ran a moment ago, and a few commands that fit what is selected — and
/// under those, the categories, one row each. Everything is reachable; nothing has to be scrolled
/// past.
/// </para>
/// <para>
/// <b>With something typed, the best match is first and the rest are in order of merit</b>
/// (<see cref="CommandMatcher"/>): how well the words matched, then whether the command can run
/// right now, then pins, recents and prominence. A command that cannot run is still listed — so
/// the reason is found rather than the command seeming not to exist — but never above one that
/// matched as well and can.
/// </para>
/// <para>
/// <b>A category narrows.</b> <c>view:</c> lists that category whole, in its own order, and
/// <c>view: sort</c> searches inside it. Choosing a category row types the token, so the list
/// teaches the syntax instead of documenting it. Any unambiguous start of a category's name will
/// do: <c>book:</c> is Bookmarks.
/// </para>
/// </remarks>
public static class PaletteRules
{
    public const int MaxRecent = 5;
    public const int MaxSuggested = 8;
    public const int MaxResults = 60;

    /// <summary>What a category row types, and what narrows to it: its name, lowercased, spaces
    /// as hyphens, then a colon.</summary>
    public static string Token(string category) => category.ToLowerInvariant().Replace(' ', '-') + ":";

    public static IReadOnlyList<PaletteRow> Build(
        string? query, IReadOnlyList<PaletteEntry> entries, PaletteState state)
    {
        var text = (query ?? "").Trim();
        var categories = Categories(entries);

        var (category, rest) = SplitCategory(text, categories);
        if (category is not null)
        {
            var within = entries.Where(e => e.Category == category).ToList();
            return rest.Length == 0
                ? [.. within.Select(e => Row(e))]
                : Ranked(rest, within, state);
        }

        return text.Length == 0 ? Unasked(entries, state, categories) : Ranked(text, entries, state);
    }

    /// <summary>
    /// The category a leading <c>name:</c> names, and what follows it. At least two letters, so a
    /// drive (<c>c:</c>) is never mistaken for one; and it has to be the start of exactly one
    /// category, since guessing between two would narrow to the wrong list silently.
    /// </summary>
    public static (string? Category, string Text) SplitCategory(string text, IReadOnlyList<string> categories)
    {
        var colon = text.IndexOf(':');
        if (colon < 2) return (null, text);

        var typed = text[..colon].Trim().Replace(' ', '-');
        if (typed.Length < 2 || typed.Any(c => !char.IsLetter(c) && c != '-')) return (null, text);

        var matches = categories
            .Where(c => Token(c).StartsWith(typed, StringComparison.OrdinalIgnoreCase))
            .ToList();

        // An exact name wins over being a prefix of several: "tabs:" is Tabs, not ambiguous
        // between Tabs and anything longer.
        var exact = matches.FirstOrDefault(c => Token(c).Equals(typed + ":", StringComparison.OrdinalIgnoreCase));
        var chosen = exact ?? (matches.Count == 1 ? matches[0] : null);

        return chosen is null ? (null, text) : (chosen, text[(colon + 1)..].Trim());
    }

    /// <summary>The categories present, in the order they first appear among the entries.</summary>
    public static IReadOnlyList<string> Categories(IReadOnlyList<PaletteEntry> entries) =>
        [.. entries.Select(e => e.Category).Distinct()];

    private static IReadOnlyList<PaletteRow> Unasked(
        IReadOnlyList<PaletteEntry> entries, PaletteState state, IReadOnlyList<string> categories)
    {
        var byId = new Dictionary<string, PaletteEntry>(StringComparer.Ordinal);
        foreach (var entry in entries) byId.TryAdd(entry.Id, entry);

        var rows = new List<PaletteRow>();
        var shown = new HashSet<string>(StringComparer.Ordinal);

        void Section(string heading, IEnumerable<string> ids, int limit, bool availableOnly)
        {
            var picked = ids
                .Where(id => !shown.Contains(id))
                .Select(id => byId.GetValueOrDefault(id))
                .OfType<PaletteEntry>()
                .Where(e => !availableOnly || e.IsAvailable)
                .Take(limit)
                .ToList();
            if (picked.Count == 0) return;

            rows.Add(new PaletteRow(PaletteRowKind.Heading, heading));
            foreach (var entry in picked)
            {
                rows.Add(Row(entry));
                shown.Add(entry.Id);
            }
        }

        // A pin is shown even when it cannot run: it was put there on purpose, and a pin that
        // came and went with the selection would be a list nobody could learn the shape of.
        Section("Pinned", state.Pins, int.MaxValue, availableOnly: false);
        // A recent that cannot run from here is noise, where a pin that cannot is a fixed point.
        Section("Recent", state.Recents, MaxRecent, availableOnly: true);
        // A suggestion that cannot run is not a suggestion.
        Section("Suggested", state.Suggested, MaxSuggested, availableOnly: true);

        rows.Add(new PaletteRow(PaletteRowKind.Heading, "All commands"));
        foreach (var category in categories)
        {
            rows.Add(new PaletteRow(
                PaletteRowKind.Category, category,
                Token: Token(category),
                Count: entries.Count(e => e.Category == category)));
        }

        return rows;
    }

    private static IReadOnlyList<PaletteRow> Ranked(
        string text, IReadOnlyList<PaletteEntry> entries, PaletteState state)
    {
        var pins = new HashSet<string>(state.Pins, StringComparer.Ordinal);
        var recents = new HashSet<string>(state.Recents, StringComparer.Ordinal);

        return
        [
            .. entries
                .Select((entry, index) => (
                    entry, index,
                    match: CommandMatcher.Match(text, entry.Name, entry.Aliases ?? [], entry.Category, entry.Gesture)))
                .Where(x => x.match.IsMatch)
                .OrderByDescending(x => x.match.Tier)
                .ThenByDescending(x => x.entry.IsAvailable)
                .ThenByDescending(x => x.match.Score + Lift(x.entry, pins, recents))
                .ThenBy(x => x.index)
                .Take(MaxResults)
                .Select(x => Row(x.entry, x.match.Highlights)),
        ];
    }

    /// <summary>What pins, recents and prominence add to a match's score — enough to order
    /// equals, never enough to lift a worse match over a better one of the same tier by much.</summary>
    private static int Lift(PaletteEntry entry, HashSet<string> pins, HashSet<string> recents)
    {
        var lift = entry.Prominence switch
        {
            CommandProminence.Essential => 6,
            CommandProminence.Rare => -6,
            _ => 0,
        };
        if (pins.Contains(entry.Id)) lift += 12;
        if (recents.Contains(entry.Id)) lift += 8;
        return lift;
    }

    private static PaletteRow Row(PaletteEntry entry, IReadOnlyList<MatchRange>? highlights = null) =>
        new(PaletteRowKind.Entry, entry.Name, entry, highlights ?? []);
}

/// <summary>
/// What an empty palette suggests, given where the window is. Curated rather than computed: the
/// point of a suggestion is that it is the thing most people would want next, and that is a
/// judgement, not a statistic — nothing here is counted or remembered.
/// </summary>
public static class PaletteSuggestions
{
    /// <summary>With something selected, the things done to a selection.</summary>
    private static readonly string[] ForSelection =
    [
        "file.rename", "pane.copy-to-other", "pane.move-to-other", "edit.copy-path", "file.compress",
        "file.extract-here", "file.properties", "file.checksum", "file.compare-files", "file.copy-to",
        "file.move-to", "file.open-terminal", "select.invert",
    ];

    /// <summary>With nothing selected, getting somewhere and seeing it differently.</summary>
    private static readonly string[] ForFolder =
    [
        "nav.address-bar", "tab.new", "pane.split-right", "select.pattern", "view.flat", "view.preview",
        "view.hidden", "file.new-folder", "search.pc", "tools.disk-usage", "tools.duplicates",
        "file.open-terminal", "view.toggle-thumbnails", "tools.compare",
    ];

    /// <summary>On the settings page, where nothing about the folders can run.</summary>
    private static readonly string[] ForSettings =
    [
        "app.settings", "app.keyboard", "app.customise-theme", "app.match-windows-theme", "search.syntax",
    ];

    public static IReadOnlyList<string> For(int selected, bool settingsOpen) =>
        settingsOpen ? ForSettings : selected > 0 ? ForSelection : ForFolder;

    /// <summary>Every id any list above names, so a test can hold them to the catalogue.</summary>
    public static IEnumerable<string> All => ForSelection.Concat(ForFolder).Concat(ForSettings).Distinct();
}
