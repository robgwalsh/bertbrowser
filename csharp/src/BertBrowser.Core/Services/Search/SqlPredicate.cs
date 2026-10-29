using System.Text;

namespace BertBrowser.Core.Services.Search;

/// <summary>
/// A WHERE fragment compiled from a query, plus the values it binds.
/// </summary>
/// <param name="Sql">The fragment, already parenthesised where it needs to be.</param>
/// <param name="Parameters">Name/value pairs the caller binds; names are unique per compile.</param>
/// <param name="Complete">
/// True when the fragment matches <em>exactly</em> what <see cref="SearchNode.Matches"/> matches.
/// False when some term could not be expressed in SQL and the fragment is therefore a
/// <em>superset</em> — see the remarks on <see cref="SearchNode"/>. A caller may only push
/// <c>LIMIT</c> into the query when this is true.
/// </param>
public readonly record struct SqlPredicate(
    string Sql,
    IReadOnlyList<KeyValuePair<string, object>> Parameters,
    bool Complete);

/// <summary>Accumulates a <see cref="SqlPredicate"/> as the node tree writes itself out.</summary>
public sealed class SqlPredicateBuilder
{
    private readonly StringBuilder _sql = new();
    private readonly List<KeyValuePair<string, object>> _parameters = new();
    private int _next;

    /// <summary>Set by any term that cannot express itself in SQL.</summary>
    public bool Incomplete { get; private set; }

    /// <summary>Appends raw SQL text.</summary>
    public SqlPredicateBuilder Append(string sql)
    {
        _sql.Append(sql);
        return this;
    }

    /// <summary>Binds a value and appends its parameter name.</summary>
    public SqlPredicateBuilder AppendParameter(object value)
    {
        var name = "@f" + _next++;
        _parameters.Add(new KeyValuePair<string, object>(name, value));
        _sql.Append(name);
        return this;
    }

    /// <summary>Records that the SQL is now a superset of the real predicate.</summary>
    public void MarkIncomplete() => Incomplete = true;

    public SqlPredicate Build() => new(_sql.ToString(), _parameters, !Incomplete);
}
