using BertBrowser.Core.Services.Commands;
using Xunit;

namespace BertBrowser.Core.Tests;

public class PaletteRulesTests
{
    private static PaletteEntry Entry(
        string id, string name, string category, string gesture = "", string? unavailable = null,
        CommandProminence prominence = CommandProminence.Standard, params string[] aliases) =>
        new(id, name, category, null, gesture, aliases, prominence, unavailable);

    private static readonly PaletteEntry[] Entries =
    [
        Entry("tab.new", "New tab", "Tabs", "Ctrl+T", prominence: CommandProminence.Essential),
        Entry("tab.close", "Close tab", "Tabs", "Ctrl+W"),
        Entry("tab.close-others", "Close other tabs", "Tabs", unavailable: "This is the only tab."),
        Entry("file.rename", "Rename", "File", "F2", unavailable: "Nothing is selected."),
        Entry("file.new-folder", "New folder", "File", "Ctrl+Shift+N"),
        Entry("view.flat", "Toggle flat view", "View", "Ctrl+B", aliases: ["branch view"]),
        Entry("view.preview", "Toggle preview pane", "View", "Ctrl+P"),
        Entry("workspace:Work", "Work", "Workspaces"),
        Entry("bookmark:C:\\Photos", "Photos", "Bookmarks"),
    ];

    private static readonly PaletteState Nothing = new([], [], []);

    private static string Show(IEnumerable<PaletteRow> rows) => string.Join(" | ", rows.Select(r => r.Kind switch
    {
        PaletteRowKind.Heading => $"# {r.Text}",
        PaletteRowKind.Category => $"{r.Token}{r.Count}",
        _ => r.Entry!.IsAvailable ? r.Text : $"({r.Text})",
    }));

    private static string Search(string query, PaletteState? state = null) =>
        Show(PaletteRules.Build(query, Entries, state ?? Nothing));

    // --- Nothing typed ---

    [Fact]
    public void An_empty_box_never_dumps_the_catalogue()
    {
        var rows = PaletteRules.Build("", Entries, Nothing);

        Assert.Equal("# All commands | tabs:3 | file:2 | view:2 | workspaces:1 | bookmarks:1", Show(rows));
        Assert.DoesNotContain(rows, r => r.Kind == PaletteRowKind.Entry);
    }

    [Fact]
    public void Pins_then_recents_then_suggestions_then_the_categories()
    {
        var state = new PaletteState(
            Pins: ["view.flat"],
            Recents: ["tab.close", "view.flat"],
            Suggested: ["tab.new", "tab.close", "file.new-folder"]);

        Assert.StartsWith(
            "# Pinned | Toggle flat view | # Recent | Close tab | # Suggested | New tab | New folder | # All commands",
            Search("", state));
    }

    [Fact]
    public void A_pin_stays_put_when_it_cannot_run_but_a_recent_or_a_suggestion_does_not_appear()
    {
        var state = new PaletteState(
            Pins: ["file.rename"], Recents: ["tab.close-others"], Suggested: ["tab.close-others", "tab.new"]);

        Assert.StartsWith("# Pinned | (Rename) | # Suggested | New tab | # All commands", Search("", state));
    }

    [Fact]
    public void Recents_and_suggestions_are_capped()
    {
        var many = Enumerable.Range(0, 30).Select(i => Entry($"x.{i}", $"Thing {i}", "X")).ToArray();
        var ids = many.Select(e => e.Id).ToList();

        var rows = PaletteRules.Build("", many, new PaletteState([], ids, ids));

        Assert.Equal(PaletteRules.MaxRecent + PaletteRules.MaxSuggested, rows.Count(r => r.Kind == PaletteRowKind.Entry));
    }

    [Fact]
    public void An_id_nothing_answers_to_is_skipped_not_fatal()
    {
        // A pin for a workspace since deleted, or a command a later build removed.
        var state = new PaletteState(Pins: ["workspace:Gone", "tab.new"], Recents: [], Suggested: []);

        Assert.StartsWith("# Pinned | New tab | # All commands", Search("", state));
    }

    // --- Something typed ---

    [Fact]
    public void The_best_match_is_first()
    {
        Assert.Equal("New tab | New folder", Search("new"));
        // The same word in three names: the two that can run lead, the commoner of them first.
        Assert.Equal("New tab | Close tab | (Close other tabs)", Search("tab"));
    }

    [Fact]
    public void A_command_that_cannot_run_is_listed_under_one_that_matched_as_well_and_can()
    {
        // Both start with "close"; the available one leads.
        Assert.Equal("Close tab | (Close other tabs)", Search("close"));
        // But it is still found by name, with its reason, rather than seeming not to exist.
        Assert.Equal("(Rename)", Search("rename"));
        Assert.Equal("Nothing is selected.",
            PaletteRules.Build("rename", Entries, Nothing).Single().Entry!.Unavailable);
    }

    [Fact]
    public void A_better_match_that_cannot_run_still_outranks_a_worse_one_that_can()
    {
        var entries = new[]
        {
            Entry("a", "Reopen closed tab", "Tabs"),
            Entry("b", "Open", "File", unavailable: "Nothing is selected."),
        };

        // "open" is the whole of one name and only the middle of the other.
        Assert.Equal("(Open) | Reopen closed tab", Show(PaletteRules.Build("open", entries, Nothing)));
    }

    [Fact]
    public void Pins_and_recents_order_equals()
    {
        Assert.Equal("Toggle flat view | Toggle preview pane", Search("toggle"));
        Assert.Equal("Toggle preview pane | Toggle flat view",
            Search("toggle", new PaletteState([], ["view.preview"], [])));
        Assert.Equal("Toggle preview pane | Toggle flat view",
            Search("toggle", new PaletteState(["view.preview"], [], [])));
    }

    [Fact]
    public void Found_by_another_word_for_it_and_by_its_shortcut()
    {
        Assert.Equal("Toggle flat view", Search("branch"));
        Assert.Equal("Toggle flat view", Search("ctrl+b"));
        Assert.Equal("New folder", Search("ctrl+shift"));
    }

    [Fact]
    public void Named_things_are_found_like_commands()
    {
        Assert.Equal("Work", Search("work"));
        Assert.Equal("Photos", Search("photo"));
    }

    [Fact]
    public void Highlights_say_where_the_name_matched()
    {
        var row = PaletteRules.Build("close", Entries, Nothing)[0];

        Assert.Equal("Close tab", row.Text);
        Assert.Equal([new MatchRange(0, 5)], row.Highlights);
    }

    // --- Narrowing ---

    [Fact]
    public void A_category_token_lists_that_category_whole_in_its_own_order()
    {
        Assert.Equal("New tab | Close tab | (Close other tabs)", Search("tabs:"));
        Assert.Equal("Work", Search("workspaces:"));
    }

    [Fact]
    public void And_text_after_it_searches_inside()
    {
        Assert.Equal("Close tab | (Close other tabs)", Search("tabs: close"));
        Assert.Equal("", Search("view: close"));
    }

    [Theory]
    [InlineData("book:", "Photos")]
    [InlineData("wo:", "Work")]
    [InlineData("VIEW:", "Toggle flat view | Toggle preview pane")]
    public void Any_unambiguous_start_of_a_category_will_do(string query, string expected)
    {
        Assert.Equal(expected, Search(query));
    }

    [Fact]
    public void A_drive_letter_is_never_a_category()
    {
        var (category, rest) = PaletteRules.SplitCategory("c:\\windows", ["Custom", "Tabs"]);

        Assert.Null(category);
        Assert.Equal("c:\\windows", rest);
    }

    [Fact]
    public void A_start_shared_by_two_categories_narrows_to_neither()
    {
        var (category, _) = PaletteRules.SplitCategory("ta: x", ["Tabs", "Tags"]);
        Assert.Null(category);

        // Unless it is one of them exactly.
        Assert.Equal("Tabs", PaletteRules.SplitCategory("tabs: x", ["Tabs", "Tabs extra"]).Category);
    }

    [Fact]
    public void A_category_row_types_its_own_token()
    {
        Assert.Equal("saved-searches:", PaletteRules.Token("Saved searches"));
        Assert.Equal("Saved searches",
            PaletteRules.SplitCategory("saved-searches: x", ["Saved searches"]).Category);
        Assert.Equal("Saved searches",
            PaletteRules.SplitCategory("saved searches: x", ["Saved searches"]).Category);
    }

    // --- Suggestions ---

    [Fact]
    public void Every_suggestion_is_a_real_command()
    {
        Assert.All(PaletteSuggestions.All, id => Assert.True(CommandCatalog.IsKnown(id), id));
    }

    [Fact]
    public void Suggestions_follow_what_is_selected_and_where_the_window_is()
    {
        Assert.Contains("file.rename", PaletteSuggestions.For(selected: 2, settingsOpen: false));
        Assert.DoesNotContain("file.rename", PaletteSuggestions.For(selected: 0, settingsOpen: false));
        Assert.Contains("nav.address-bar", PaletteSuggestions.For(selected: 0, settingsOpen: false));
        Assert.DoesNotContain("tab.new", PaletteSuggestions.For(selected: 0, settingsOpen: true));
    }

    [Fact]
    public void Every_suggestion_on_the_settings_page_works_there()
    {
        Assert.All(PaletteSuggestions.For(selected: 0, settingsOpen: true),
            id => Assert.Equal(CommandContext.App, CommandCatalog.Find(id)!.Context));
    }
}
