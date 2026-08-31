namespace BertBrowser.Core.Services.Search;

/// <summary>
/// One node of a parsed query: a leaf term, or an AND/OR/NOT over other nodes.
/// </summary>
/// <remarks>
/// <para><strong>Two abstract members, deliberately.</strong> A node has to answer both
/// "does this entry match?" and "how do I say that in SQL?", so a new filter key cannot be
/// added with only one side wired up — it is a compile error rather than a silent difference
/// between an indexed drive and a live scan. This is the "one rule, two consumers" discipline
/// <c>NavigationRequest.IsAcceptablePath</c> is cited for, made structural.</para>
/// <para><strong><see cref="Matches"/> is the definition; the SQL is only an optimisation.</strong>
/// <c>FsIndexRepository</c> re-applies <see cref="Matches"/> to every row it reads back, so a
/// <see cref="WriteSql"/> that is too <em>wide</em> costs a slower scan and nothing else. One
/// that is too <em>narrow</em> drops rows, and the agreement test goes red. Terms that cannot be
/// expressed at all (<c>re:</c>) are therefore free to emit <c>1</c>.</para>
/// </remarks>
public abstract class SearchNode
{
    /// <summary>Whether <paramref name="candidate"/> satisfies this node. The definition.</summary>
    /// <remarks>
    /// Three-valued rather than boolean, because a <c>content:</c> term cannot be answered from an
    /// index row or a directory entry — see <see cref="SearchMatch"/>. Every other term is
    /// <see cref="SearchMatch.Yes"/> or <see cref="SearchMatch.No"/> and always was.
    /// </remarks>
    public abstract SearchMatch Matches(in SearchCandidate candidate);

    /// <summary>
    /// Writes a WHERE fragment that matches <em>at least</em> everything <see cref="Matches"/>
    /// does. A node that cannot be expressed calls <see cref="SqlPredicateBuilder.MarkIncomplete"/>
    /// and emits <c>1</c>.
    /// </summary>
    public abstract void WriteSql(SqlPredicateBuilder builder);

    /// <summary>
    /// Whether this node's SQL is exact rather than a superset. Read only by <see cref="Not"/>,
    /// which cannot negate a superset — see the remarks there.
    /// </summary>
    public abstract bool SqlComplete { get; }

    /// <summary>
    /// How many literal (non-wildcard) characters this subtree's text terms contribute.
    /// </summary>
    /// <remarks>
    /// A <em>sum</em> across the whole query, not a per-term test, because that is the rule the
    /// search box has always had: two single-character terms clear the floor together where
    /// either alone would not.
    /// </remarks>
    public abstract int LiteralChars { get; }

    /// <summary>
    /// Whether this subtree carries a filter specific enough to be worth running with no text
    /// at all. An extension, a real size bound, a date or a regular expression are; a kind or a
    /// hidden flag are not — half the disk is not a search result.
    /// </summary>
    public abstract bool HasFilter { get; }

    /// <summary>Whether this node reads a field the sizeless index build never filled.</summary>
    public abstract bool NeedsMetadata { get; }

    /// <summary>Whether any node in this subtree asks for hidden entries (<c>is:hidden</c>).</summary>
    public virtual bool WantsHidden => false;

    /// <summary>
    /// Whether any node in this subtree asks for archive contents to be searched too
    /// (<c>in:archives</c>).
    /// </summary>
    /// <remarks>
    /// A <em>scope</em> rather than a predicate, exactly as <see cref="WantsHidden"/> is: the index
    /// holds nothing about what is inside a container, so this cannot be answered by matching a row
    /// — it has to change which rows exist to be matched. <see cref="SearchService"/> reads it and
    /// runs a second, opt-in pass.
    /// </remarks>
    public virtual bool WantsArchives => false;

    /// <summary>Whether any node in this subtree asks for file contents (<c>content:</c>).</summary>
    /// <remarks>
    /// <para><strong>This one <em>does</em> propagate through a <see cref="NotNode"/>, unlike
    /// <see cref="WantsHidden"/> and <see cref="WantsArchives"/> — which is the asymmetry a reader
    /// will trip on, so here is the reason.</strong> Those two are requests to <em>widen</em> a
    /// scan, and <c>!is:hidden</c> or <c>!in:archives</c> asks for the default rather than for more
    /// work. But <c>!content:x</c> is not a request for less work: deciding that a file does
    /// <em>not</em> contain something needs it read exactly as much as deciding that it does.</para>
    /// <para>It is a static property of the query, never of a candidate. Its job is to decide
    /// whether the second pass runs at all, and to refuse the places that cannot run one — the
    /// correctness of <see cref="Matches"/> is <see cref="SearchMatch"/>'s job, not this.</para>
    /// </remarks>
    public virtual bool NeedsContent => false;

    /// <summary>Adds every <c>content:</c> needle in this subtree to <paramref name="into"/>.</summary>
    /// <remarks>The scanner highlights whichever needle it finds first, so it has to know what to
    /// look for without taking the tree apart itself — and collecting them here means a needle
    /// cannot be missed because it sat under a bracket or a <c>NOT</c>.</remarks>
    public virtual void CollectContentNeedles(List<string> into) { }

    /// <summary>A settled yes-or-no, for the terms that never need to read anything.</summary>
    protected static SearchMatch Verdict(bool matched) => matched ? SearchMatch.Yes : SearchMatch.No;
}

/// <summary>Every child must match.</summary>
public sealed class AndNode : SearchNode
{
    public IReadOnlyList<SearchNode> Children { get; }

    public AndNode(IReadOnlyList<SearchNode> children) => Children = children;

    /// <summary>
    /// One settled <see cref="SearchMatch.No"/> refuses the whole conjunction without reading
    /// anything — which is what makes <c>is:dir content:x</c> cost nothing instead of handing every
    /// directory in the tree to a file opener.
    /// </summary>
    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        // No LINQ and no closure: this runs once per entry of a whole-subtree walk, and
        // `in` parameters cannot be captured by a lambda anyway.
        var undecided = false;
        for (var i = 0; i < Children.Count; i++)
        {
            switch (Children[i].Matches(candidate))
            {
                case SearchMatch.No: return SearchMatch.No;
                case SearchMatch.NeedsContent: undecided = true; break;
            }
        }
        return undecided ? SearchMatch.NeedsContent : SearchMatch.Yes;
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.Append("(");
        for (var i = 0; i < Children.Count; i++)
        {
            if (i > 0) builder.Append(" AND ");
            Children[i].WriteSql(builder);
        }
        builder.Append(")");
    }

    public override bool SqlComplete => Children.All(c => c.SqlComplete);
    public override int LiteralChars => Children.Sum(c => c.LiteralChars);
    public override bool HasFilter => Children.Any(c => c.HasFilter);
    public override bool NeedsMetadata => Children.Any(c => c.NeedsMetadata);
    public override bool WantsHidden => Children.Any(c => c.WantsHidden);

    public override bool WantsArchives => Children.Any(c => c.WantsArchives);

    public override bool NeedsContent => Children.Any(c => c.NeedsContent);

    public override void CollectContentNeedles(List<string> into)
    {
        for (var i = 0; i < Children.Count; i++) Children[i].CollectContentNeedles(into);
    }
}

/// <summary>Any child may match.</summary>
public sealed class OrNode : SearchNode
{
    public IReadOnlyList<SearchNode> Children { get; }

    public OrNode(IReadOnlyList<SearchNode> children) => Children = children;

    /// <summary>
    /// One settled <see cref="SearchMatch.Yes"/> satisfies the whole disjunction without reading
    /// anything. That is the other half of the optimisation: in <c>content:a OR ext:txt</c> every
    /// <c>.txt</c> is a hit already, and returning "undecided" for it would spend the file budget
    /// re-establishing what the name had settled.
    /// </summary>
    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        var undecided = false;
        for (var i = 0; i < Children.Count; i++)
        {
            switch (Children[i].Matches(candidate))
            {
                case SearchMatch.Yes: return SearchMatch.Yes;
                case SearchMatch.NeedsContent: undecided = true; break;
            }
        }
        return undecided ? SearchMatch.NeedsContent : SearchMatch.No;
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.Append("(");
        for (var i = 0; i < Children.Count; i++)
        {
            if (i > 0) builder.Append(" OR ");
            Children[i].WriteSql(builder);
        }
        builder.Append(")");
    }

    public override bool SqlComplete => Children.All(c => c.SqlComplete);

    public override int LiteralChars => Children.Sum(c => c.LiteralChars);
    public override bool HasFilter => Children.Any(c => c.HasFilter);

    public override bool NeedsMetadata => Children.Any(c => c.NeedsMetadata);
    public override bool WantsHidden => Children.Any(c => c.WantsHidden);

    public override bool WantsArchives => Children.Any(c => c.WantsArchives);

    public override bool NeedsContent => Children.Any(c => c.NeedsContent);

    public override void CollectContentNeedles(List<string> into)
    {
        for (var i = 0; i < Children.Count; i++) Children[i].CollectContentNeedles(into);
    }
}

/// <summary>The child must not match.</summary>
public sealed class NotNode : SearchNode
{
    public SearchNode Child { get; }

    public NotNode(SearchNode child) => Child = child;

    /// <summary>
    /// Negation flips a settled answer and leaves an unsettled one alone.
    /// </summary>
    /// <remarks>
    /// <strong>This is the counterpart of the rule <see cref="WriteSql"/> keeps below</strong> — a
    /// superset cannot be negated — and here it costs nothing to state, because "undecided" is a
    /// value rather than a guess. Map <see cref="SearchMatch.NeedsContent"/> to either
    /// <see cref="SearchMatch.Yes"/> or <see cref="SearchMatch.No"/> instead and
    /// <c>!content:foo</c> breaks: to <c>No</c> and the first pass yields nothing at all, to
    /// <c>Yes</c> and the reader is never asked and every file is reported as a hit.
    /// </remarks>
    public override SearchMatch Matches(in SearchCandidate candidate) => Child.Matches(candidate) switch
    {
        SearchMatch.Yes => SearchMatch.No,
        SearchMatch.No => SearchMatch.Yes,
        _ => SearchMatch.NeedsContent,
    };

    /// <summary>
    /// <strong>A superset cannot be negated.</strong> Every <see cref="WriteSql"/> is allowed to
    /// be wider than its <see cref="Matches"/>; negating a wider set gives a <em>narrower</em>
    /// one, which would drop rows that really do match. So when the child's SQL is inexact this
    /// emits <c>1</c> and leans on the repository's re-check instead. Without this,
    /// <c>!re:foo</c> compiles to <c>NOT 1</c> and returns nothing at all.
    /// </summary>
    public override void WriteSql(SqlPredicateBuilder builder)
    {
        if (!Child.SqlComplete)
        {
            builder.MarkIncomplete();
            builder.Append("1");
            return;
        }

        builder.Append("NOT ");
        Child.WriteSql(builder);
    }

    public override bool SqlComplete => Child.SqlComplete;

    /// <summary>An exclusion never narrows — <c>!tmp</c> alone is "almost everything" — so its
    /// contents contribute nothing towards clearing the floor.</summary>
    public override int LiteralChars => 0;

    public override bool HasFilter => false;

    public override bool NeedsMetadata => Child.NeedsMetadata;

    /// <summary>Deliberately not the child's: <c>!is:hidden</c> asks to exclude hidden entries,
    /// which is the default, not a request to widen the scan.</summary>
    public override bool WantsHidden => false;

    /// <summary>Deliberately not the child's either, and for the same reason: <c>!in:archives</c>
    /// asks to leave containers out, which is already what happens.</summary>
    public override bool WantsArchives => false;

    /// <summary>Unlike the two above, this one <em>does</em> take the child's: establishing that a
    /// file does not contain something needs it read exactly as much as establishing that it does.
    /// See the remarks on <see cref="SearchNode.NeedsContent"/>.</summary>
    public override bool NeedsContent => Child.NeedsContent;

    public override void CollectContentNeedles(List<string> into) => Child.CollectContentNeedles(into);
}
