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
    public abstract bool Matches(in SearchCandidate candidate);

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
}

/// <summary>Every child must match.</summary>
public sealed class AndNode : SearchNode
{
    public IReadOnlyList<SearchNode> Children { get; }

    public AndNode(IReadOnlyList<SearchNode> children) => Children = children;

    public override bool Matches(in SearchCandidate candidate)
    {
        // No LINQ and no closure: this runs once per entry of a whole-subtree walk, and
        // `in` parameters cannot be captured by a lambda anyway.
        for (var i = 0; i < Children.Count; i++)
            if (!Children[i].Matches(candidate))
                return false;
        return true;
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
}

/// <summary>Any child may match.</summary>
public sealed class OrNode : SearchNode
{
    public IReadOnlyList<SearchNode> Children { get; }

    public OrNode(IReadOnlyList<SearchNode> children) => Children = children;

    public override bool Matches(in SearchCandidate candidate)
    {
        for (var i = 0; i < Children.Count; i++)
            if (Children[i].Matches(candidate))
                return true;
        return false;
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
}

/// <summary>The child must not match.</summary>
public sealed class NotNode : SearchNode
{
    public SearchNode Child { get; }

    public NotNode(SearchNode child) => Child = child;

    public override bool Matches(in SearchCandidate candidate) => !Child.Matches(candidate);

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
}
