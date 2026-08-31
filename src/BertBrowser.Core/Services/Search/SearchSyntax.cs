namespace BertBrowser.Core.Services.Search;

/// <summary>One line of the search syntax help.</summary>
/// <param name="Example">What to type.</param>
/// <param name="Meaning">What it does.</param>
public readonly record struct SearchSyntaxEntry(string Example, string Meaning);

/// <summary>A group of related help lines, with a heading.</summary>
/// <param name="Title">The heading.</param>
/// <param name="Blurb">One line of orientation under the heading.</param>
/// <param name="Entries">The lines themselves.</param>
public sealed record SearchSyntaxSection(
    string Title, string Blurb, IReadOnlyList<SearchSyntaxEntry> Entries);

/// <summary>
/// The filter keys the query language understands, and the help text describing them.
/// </summary>
/// <remarks>
/// <para>Parser and UI read the same table, so the popover cannot advertise a key the parser
/// does not implement — the reason <c>ResolvedNewFileTypes</c> is the one place the New menu,
/// the settings page and the harness all resolve file types from.</para>
/// <para><strong>An unrecognised key is a name term, not an error.</strong> Colons are legal in
/// what people type at a file browser — <c>C:\Users</c> pasted into the box has to keep meaning
/// a search for that text. Only the keys named here take on a second meaning, which is the same
/// trade the advanced rename made by treating braces as tokens solely in advanced mode.</para>
/// </remarks>
public static class SearchSyntax
{
    // --- Recognised keys, uppercased. ---

    public const string Extension = "EXT";
    public const string Path = "PATH";
    public const string Size = "SIZE";
    public const string Modified = "DM";
    public const string Is = "IS";
    public const string Regex = "RE";
    public const string Name = "NAME";
    public const string In = "IN";

    /// <summary>Aliases mapped onto the canonical key above.</summary>
    private static readonly Dictionary<string, string> Aliases = new(StringComparer.Ordinal)
    {
        ["EXT"] = Extension,
        ["EXTENSION"] = Extension,
        ["PATH"] = Path,
        ["SIZE"] = Size,
        ["DM"] = Modified,
        ["MODIFIED"] = Modified,
        ["DATEMODIFIED"] = Modified,
        ["IS"] = Is,
        ["RE"] = Regex,
        ["REGEX"] = Regex,
        ["NAME"] = Name,
        ["IN"] = In,
    };

    /// <summary>
    /// Keys that plainly mean something but that this index cannot answer, mapped to why.
    /// </summary>
    /// <remarks>
    /// These are refused with a message rather than degraded to a name term. Someone typing
    /// a created-date filter has a specific question; silently searching for the literal text
    /// "dc:today" answers a different one and returns nothing, which reads as "no such files"
    /// rather than "no such filter". <c>fs_entry</c> stores modified time only, and
    /// <c>docs/search-indexing.md</c> puts content indexing out of scope.
    /// </remarks>
    private static readonly Dictionary<string, string> Unsupported = new(StringComparer.Ordinal)
    {
        ["DC"] = "created date isn't indexed — only modified is, so use dm:",
        ["DATECREATED"] = "created date isn't indexed — only modified is, so use dm:",
        ["DA"] = "accessed date isn't indexed — only modified is, so use dm:",
        ["DATEACCESSED"] = "accessed date isn't indexed — only modified is, so use dm:",
        ["CONTENT"] = "file contents aren't indexed — this searches names only",
        ["CONTENTS"] = "file contents aren't indexed — this searches names only",
    };

    /// <summary>Resolves a typed key to its canonical form, or null when it is not a key at all.</summary>
    public static string? Resolve(string key) =>
        Aliases.TryGetValue(key, out var canonical) ? canonical : null;

    /// <summary>Why a key cannot be used, or null when it is fine or is not a known key.</summary>
    public static string? UnsupportedReason(string key) =>
        Unsupported.TryGetValue(key, out var reason) ? reason : null;

    /// <summary>The help, grouped, in the order it should be shown.</summary>
    public static IReadOnlyList<SearchSyntaxSection> Sections { get; } = new SearchSyntaxSection[]
    {
        new("Words", "Type anything. Several words all have to match, in any order.",
        [
            new("report 2026", "names containing both words"),
            new("\"annual report\"", "an exact phrase, spaces and all"),
            new("*.log", "* matches any run, ? matches one character"),
        ]),

        new("Filters", "Narrow by something other than the name.",
        [
            new("ext:jpg;png", "one of these extensions"),
            new("size:>100mb", "also <, >=, <=, =, 1mb..2gb and empty"),
            new("dm:today", "also yesterday, thisweek, last7days, 2026-08"),
            new("path:projects", "somewhere in the folder path, not just the name"),
            new("is:dir", "folders only — also is:file and is:hidden"),
            new("re:^IMG_\\d+", "a regular expression over the name"),
            new("in:archives", "look inside zips and 7zs too — names, not contents"),
        ]),

        new("Combining", "Words sit side by side to mean AND; the rest is spelled out.",
        [
            new("!tmp", "exclude — NOT works the same way"),
            new("draft OR final", "either one; OR must be uppercase"),
            new("(a OR b) ext:txt", "brackets group"),
        ]),

        new("Worth knowing", "Where the answers come from.",
        [
            new("size: and dm:", "need a drive the indexer read in full"),
            new("dc: and da:", "aren't indexed — only the modified date is"),
            new("content:", "file contents are never searched, only names"),
            new("in:archives", "opens each archive, so it is slower and opt-in"),
        ]),
    };

    /// <summary>The same help flattened, for the tooltip. Derived, so the two cannot disagree.</summary>
    public static IReadOnlyList<SearchSyntaxEntry> Entries { get; } =
        Sections.SelectMany(s => s.Entries).ToArray();
}
