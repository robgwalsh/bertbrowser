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
    /// <remarks>
    /// <strong>Undecided counts as a match here, and that is what keeps this a superset.</strong>
    /// The three callers — the repository's row re-check, the live scan, and the archive scanner —
    /// are all asking "could this be a hit?", and a candidate whose file has not been read yet
    /// could be. They are unchanged by <c>content:</c> existing. Anything that means to act on the
    /// answer, rather than to shortlist on it, asks <see cref="Evaluate"/> instead.
    /// </remarks>
    public bool Matches(in SearchCandidate candidate) => _root.Matches(candidate) != SearchMatch.No;

    /// <summary>
    /// The full three-valued verdict: a hit, not a hit, or not answerable until the file is read.
    /// </summary>
    /// <remarks>
    /// Only the content pass wants this. It is what lets the scanner spend its file budget solely
    /// on <see cref="SearchMatch.NeedsContent"/> candidates and take a
    /// <see cref="SearchMatch.Yes"/> straight to the results without opening anything.
    /// </remarks>
    public SearchMatch Evaluate(in SearchCandidate candidate) => _root.Matches(candidate);

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

    /// <summary>Whether the query asks for file contents, and so needs a second, reading pass.</summary>
    /// <remarks>Also the flag the places that <em>cannot</em> run that pass refuse on: an archive
    /// interior has no file on disk to open, so answering there would report every entry as a hit.
    /// </remarks>
    public bool NeedsContent => _root.NeedsContent;

    /// <summary>Every <c>content:</c> needle in the query, in tree order.</summary>
    /// <remarks>What the scanner highlights. Built once per query rather than per file — there are
    /// tens of thousands of the latter.</remarks>
    public IReadOnlyList<string> ContentNeedles
    {
        get
        {
            var needles = new List<string>();
            _root.CollectContentNeedles(needles);
            return needles;
        }
    }
}
