namespace BertBrowser.Core.Services.Search;

/// <summary>
/// A parsed search query: a tree of terms, ANDed by adjacency, with <c>OR</c>, <c>!</c>,
/// brackets, quoted phrases and the filter keys in <see cref="SearchSyntax"/>.
/// </summary>
/// <remarks>
/// <para>Bare words keep the meaning they have always had — a case-insensitive substring of the
/// entry name, with <c>*</c> and <c>?</c> honoured, several of them ANDed. Everything else is
/// additive, and an unrecognised <c>key:</c> stays a bare word.</para>
/// <para>The two faces of a query — <see cref="Matches"/> and <see cref="Compile"/> — are
/// deliberately not independent implementations: see <see cref="SearchNode"/> for why the SQL
/// is only ever an optimisation over the matcher.</para>
/// </remarks>
public sealed class SearchQuery
{
    private readonly SearchNode _root;

    internal SearchQuery(SearchNode root, string text)
    {
        _root = root;
        Text = text;
    }

    /// <summary>The text this was parsed from, as typed.</summary>
    public string Text { get; }

    /// <summary>
    /// Parses query text. Never throws; see <see cref="SearchQueryParse"/> for how "nothing to
    /// run" and "this cannot be used" differ.
    /// </summary>
    public static SearchQueryParse Parse(string? text) => SearchGrammar.Parse(text);

    /// <summary>Whether <paramref name="candidate"/> satisfies the query. The definition of a hit.</summary>
    public bool Matches(in SearchCandidate candidate) => _root.Matches(candidate);

    /// <summary>
    /// The WHERE fragment for an index query. Matches at least everything <see cref="Matches"/>
    /// does; when <see cref="SqlPredicate.Complete"/> is false it matches strictly more, and the
    /// caller must re-check every row and must not push <c>LIMIT</c> down.
    /// </summary>
    public SqlPredicate Compile()
    {
        var builder = new SqlPredicateBuilder();
        _root.WriteSql(builder);
        return builder.Build();
    }

    /// <summary>
    /// True when the query asks for hidden entries. Search otherwise excludes them outright —
    /// hidden entries are index noise that buries real results — so this is what stops
    /// <c>is:hidden</c> being filtered away to nothing by the very caller that runs it.
    /// </summary>
    public bool WantsHidden => _root.WantsHidden;

    /// <summary>Whether the query asked for archive contents to be scanned as well.</summary>
    public bool WantsArchives => _root.WantsArchives;

    /// <summary>
    /// True when the query filters on size or modified time. Those columns are empty on a volume
    /// the indexer had to build through its <c>FSCTL_ENUM_USN_DATA</c> fallback, which records
    /// names only — so a query with this set that comes back empty may be unanswerable rather
    /// than genuinely unmatched, and the caller says so instead of showing a bare "no results".
    /// </summary>
    public bool NeedsMetadata => _root.NeedsMetadata;
}
