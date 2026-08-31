using System.Text.RegularExpressions;

namespace BertBrowser.Core.Services.Search;

/// <summary>A substring of the entry name, with <c>*</c> and <c>?</c> honoured. The bare-word
/// term, and what every query meant before filters existed.</summary>
public sealed class NameTerm : SearchNode
{
    private readonly string _pattern;   // "*TERM*" for the in-memory matcher
    private readonly string _glob;      // "*TERM*" with '[' escaped, for SQLite

    /// <param name="term">Uppercased, wildcards preserved.</param>
    /// <param name="literal">True for a quoted phrase: <c>*</c> and <c>?</c> lose their meaning.</param>
    public NameTerm(string term, bool literal = false)
    {
        Term = term;
        Literal = literal;
        _pattern = "*" + (literal ? GlobText.Escape(term, keepWildcards: false) : term) + "*";
        _glob = "*" + GlobText.Escape(term, keepWildcards: !literal) + "*";
    }

    public string Term { get; }
    public bool Literal { get; }

    public override SearchMatch Matches(in SearchCandidate candidate) => Verdict(
        Literal
            ? candidate.NameKey.Contains(Term, StringComparison.Ordinal)
            : GlobText.WildcardMatch(candidate.NameKey, _pattern));

    public override void WriteSql(SqlPredicateBuilder builder) =>
        builder.Append("name_key GLOB ").AppendParameter(_glob);

    public override bool SqlComplete => true;

    /// <summary>Two literal characters across the whole query is the floor a bare term has
    /// always had to help clear.</summary>
    public override int LiteralChars => Term.Count(c => c is not ('*' or '?'));
    public override bool HasFilter => false;

    public override bool NeedsMetadata => false;
}

/// <summary>A substring of the entry's full path.</summary>
public sealed class PathTerm : SearchNode
{
    private readonly string _pattern;
    private readonly string _glob;

    public PathTerm(string term)
    {
        Term = term;
        _pattern = "*" + term + "*";
        _glob = "*" + GlobText.Escape(term, keepWildcards: true) + "*";
    }

    public string Term { get; }

    public override SearchMatch Matches(in SearchCandidate candidate) =>
        Verdict(GlobText.WildcardMatch(candidate.PathKey, _pattern));

    public override void WriteSql(SqlPredicateBuilder builder) =>
        builder.Append("path_key GLOB ").AppendParameter(_glob);

    public override bool SqlComplete => true;
    public override int LiteralChars => Term.Count(c => c is not ('*' or '?'));
    public override bool HasFilter => false;
    public override bool NeedsMetadata => false;
}

/// <summary>One of a set of extensions.</summary>
public sealed class ExtensionTerm : SearchNode
{
    /// <param name="extensions">Uppercased, without the dot.</param>
    public ExtensionTerm(IReadOnlyList<string> extensions) => Extensions = extensions;

    public IReadOnlyList<string> Extensions { get; }

    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        var name = candidate.NameKey;
        for (var i = 0; i < Extensions.Count; i++)
        {
            var ext = Extensions[i];
            // A file *called* ".jpg" has no extension, it has a dotted name — so require at
            // least one character before the dot.
            if (name.Length > ext.Length + 1
                && name[name.Length - ext.Length - 1] == '.'
                && name.EndsWith(ext, StringComparison.Ordinal))
                return SearchMatch.Yes;
        }
        return SearchMatch.No;
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.Append("(");
        for (var i = 0; i < Extensions.Count; i++)
        {
            if (i > 0) builder.Append(" OR ");
            builder.Append("name_key GLOB ")
                   .AppendParameter("?*." + GlobText.Escape(Extensions[i], keepWildcards: false));
        }
        builder.Append(")");
    }

    public override bool SqlComplete => true;
    public override int LiteralChars => 0;
    public override bool HasFilter => true;
    public override bool NeedsMetadata => false;
}

/// <summary>A byte-length range, half-open <c>[Lo, Hi)</c>. Either bound may be absent.</summary>
/// <remarks>
/// <strong>Only files.</strong> A directory's <c>size_bytes</c> is 0 in the index — recursive
/// totals live in <c>dir_size_cache</c>, which is a different table and a different question —
/// so letting folders through would make a small-size filter return every folder on the disk.
/// </remarks>
public sealed class SizeTerm : SearchNode
{
    public SizeTerm(long? lo, long? hi)
    {
        Lo = lo;
        Hi = hi;
    }

    public long? Lo { get; }
    public long? Hi { get; }

    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        if (candidate.IsDirectory) return SearchMatch.No;
        if (Lo is { } lo && candidate.SizeBytes < lo) return SearchMatch.No;
        if (Hi is { } hi && candidate.SizeBytes >= hi) return SearchMatch.No;
        return SearchMatch.Yes;
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.Append("(is_dir = 0");
        if (Lo is { } lo) builder.Append(" AND size_bytes >= ").AppendParameter(lo);
        if (Hi is { } hi) builder.Append(" AND size_bytes < ").AppendParameter(hi);
        builder.Append(")");
    }

    public override bool SqlComplete => true;

    public override int LiteralChars => 0;

    /// <summary>A bound that excludes nothing is not a filter — "at least zero bytes" is everything.</summary>
    public override bool HasFilter => Lo is > 0 || Hi is not null;

    public override bool NeedsMetadata => true;
}

/// <summary>A modified-time range, half-open <c>[Lo, Hi)</c> in UTC.</summary>
public sealed class DateTerm : SearchNode
{
    /// <summary>
    /// Below any real filesystem timestamp (NTFS counts from 1601) and above the
    /// <see cref="DateTime.MinValue"/> that the sizeless index build writes. Rows carrying that
    /// sentinel have no timestamp at all and must not satisfy a date filter — including an
    /// open-ended one like "before 2020", which they would otherwise all match.
    /// </summary>
    internal static readonly DateTime Epoch = new(1601, 1, 1, 0, 0, 0, DateTimeKind.Utc);

    public DateTerm(DateTime? lo, DateTime? hi)
    {
        Lo = lo;
        Hi = hi;
    }

    public DateTime? Lo { get; }
    public DateTime? Hi { get; }

    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        if (candidate.ModifiedUtc < Epoch) return SearchMatch.No;
        if (Lo is { } lo && candidate.ModifiedUtc < lo) return SearchMatch.No;
        if (Hi is { } hi && candidate.ModifiedUtc >= hi) return SearchMatch.No;
        return SearchMatch.Yes;
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        // modified_utc is TEXT written with "O" — fixed-width and zero-padded, so BINARY
        // collation is already a correct chronological order and these are string comparisons.
        builder.Append("(modified_utc >= ").AppendParameter(Format(Lo ?? Epoch));
        if (Hi is { } hi) builder.Append(" AND modified_utc < ").AppendParameter(Format(hi));
        builder.Append(")");
    }

    private static string Format(DateTime value) =>
        DateTime.SpecifyKind(value, DateTimeKind.Utc).ToString("O");

    public override bool SqlComplete => true;
    public override int LiteralChars => 0;
    public override bool HasFilter => true;
    public override bool NeedsMetadata => true;
}

/// <summary>Directory or file.</summary>
public sealed class KindTerm : SearchNode
{
    public KindTerm(bool isDirectory) => IsDirectory = isDirectory;

    public bool IsDirectory { get; }

    public override SearchMatch Matches(in SearchCandidate candidate) =>
        Verdict(candidate.IsDirectory == IsDirectory);

    public override void WriteSql(SqlPredicateBuilder builder) =>
        builder.Append("is_dir = ").AppendParameter(IsDirectory ? 1 : 0);

    public override bool SqlComplete => true;

    /// <summary>Half the disk is not a search result. This is exactly the case the
    /// two-literal-character floor was written for, so it must not run on its own.</summary>
    public override int LiteralChars => 0;
    public override bool HasFilter => false;

    public override bool NeedsMetadata => false;
}

/// <summary>Hidden entries — and the reason <see cref="SearchNode.WantsHidden"/> exists.</summary>
public sealed class HiddenTerm : SearchNode
{
    public override SearchMatch Matches(in SearchCandidate candidate) => Verdict(candidate.Hidden);

    public override void WriteSql(SqlPredicateBuilder builder) => builder.Append("hidden = 1");

    public override bool SqlComplete => true;
    public override int LiteralChars => 0;
    public override bool HasFilter => false;
    public override bool NeedsMetadata => false;

    /// <summary>Search otherwise excludes hidden entries outright, so without this the term
    /// would be filtered away by the caller and read as a broken feature.</summary>
    public override bool WantsHidden => true;
}

/// <summary>
/// <c>in:archives</c> — widen the search to the contents of archives under the root.
/// </summary>
/// <remarks>
/// <para>
/// A marker, and both abstract members are wired even though neither does any work:
/// <see cref="Matches"/> is <c>true</c> and the SQL is <c>1</c>, because this term does not decide
/// whether a <em>row</em> qualifies. It decides which rows exist at all, and that is
/// <see cref="SearchNode.WantsArchives"/>'s job.
/// </para>
/// <para>
/// The vacuous <c>Matches</c> is not a smell here for the same reason <c>is:hidden</c>'s
/// <c>WantsHidden</c> is not: this is a scope. Making it a predicate instead would mean the index
/// having a column for something it has never held.
/// </para>
/// </remarks>
public sealed class InArchivesTerm : SearchNode
{
    public override SearchMatch Matches(in SearchCandidate candidate) => SearchMatch.Yes;

    public override void WriteSql(SqlPredicateBuilder builder) => builder.Append("1");

    /// <summary>
    /// A superset, so a <c>LIMIT</c> must not be pushed down past it and a <c>NOT</c> must not
    /// invert it — both of which the existing rules already handle once this says so.
    /// </summary>
    public override bool SqlComplete => false;

    public override int LiteralChars => 0;

    /// <summary>Scope, not a filter: "everything, but also look in archives" is not a search.</summary>
    public override bool HasFilter => false;

    public override bool NeedsMetadata => false;

    public override bool WantsArchives => true;
}

/// <summary>A regular expression over the entry name.</summary>
/// <remarks>
/// The one term with no SQL at all. It emits <c>1</c> and marks the predicate incomplete, which
/// is safe because the repository re-applies <see cref="Matches"/> to every row it reads; the
/// cost is that SQLite cannot stop early, so the caller must not push <c>LIMIT</c> down.
/// </remarks>
public sealed class RegexTerm : SearchNode
{
    private readonly Regex _regex;

    public RegexTerm(Regex regex) => _regex = regex;

    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        try
        {
            return Verdict(_regex.IsMatch(candidate.NameKey));
        }
        catch (RegexMatchTimeoutException)
        {
            // A pattern that blows its budget on one name must not abort the whole search.
            return SearchMatch.No;
        }
    }

    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.MarkIncomplete();
        builder.Append("1");
    }

    public override bool SqlComplete => false;
    public override int LiteralChars => 0;
    public override bool HasFilter => true;
    public override bool NeedsMetadata => false;
}

/// <summary>Text somewhere inside the file — the one term that reads the disk.</summary>
/// <remarks>
/// <para>The needle keeps the case it was typed in, unlike every other value-taking key, which
/// <c>SearchGrammar</c> folds on the way in. Those fold because the index stores names folded and
/// the comparison is then free; here the other side is a megabyte of file text that would have to
/// be folded per file, per thread. <see cref="ContentText.IndexOf"/> compares
/// <c>OrdinalIgnoreCase</c> instead, which is measurably the same speed as an ordinal compare and
/// allocates nothing.</para>
/// <para>Wildcards are deliberately not honoured. <c>*</c> and <c>?</c> are ordinary characters in
/// file contents far more often than they are in names, so a quoted phrase and a bare word mean the
/// same thing here — the text, literally.</para>
/// </remarks>
public sealed class ContentTerm : SearchNode
{
    /// <param name="needle">As typed. Case is preserved and ignored when comparing.</param>
    public ContentTerm(string needle) => Needle = needle;

    public string Needle { get; }

    /// <summary>
    /// A directory is settled <see cref="SearchMatch.No"/> — it has no contents to search, and
    /// saying so here rather than in the scanner is what keeps <c>content:x OR is:dir</c> returning
    /// folders while <c>is:dir content:x</c> costs nothing. Unread content is
    /// <see cref="SearchMatch.NeedsContent"/>; that is the whole reason the verdict is three-valued.
    /// </summary>
    public override SearchMatch Matches(in SearchCandidate candidate)
    {
        if (candidate.IsDirectory) return SearchMatch.No;
        if (candidate.Content is not { } content) return SearchMatch.NeedsContent;
        return Verdict(content.IndexOf(Needle) >= 0);
    }

    /// <summary>
    /// No index column holds file text, so this is the widest possible predicate — the same answer
    /// <c>re:</c> gives, and for the same reason.
    /// </summary>
    /// <remarks>
    /// <strong>It marks the predicate incomplete, where <see cref="InArchivesTerm"/> does not</strong>,
    /// and the difference is the whole candidate budget. That term's <c>1</c> is <em>exact</em> —
    /// it really does match every filesystem row — so a <c>LIMIT</c> may be pushed down past it.
    /// This one's is a superset: rows pass the SQL and then fail the read. Push <c>LIMIT</c> down
    /// past it and the scan stops at the first thousand rows it <em>reached</em> rather than the
    /// candidates that survived, so the reader is handed a fraction of what it asked for and the
    /// results look arbitrary.
    /// </remarks>
    public override void WriteSql(SqlPredicateBuilder builder)
    {
        builder.MarkIncomplete();
        builder.Append("1");
    }

    /// <summary>A superset, so <c>LIMIT</c> must not be pushed down past it and a <c>NOT</c> must
    /// not invert it. Both rules already exist and take effect once this says so.</summary>
    public override bool SqlComplete => false;

    /// <summary>
    /// The needle counts towards the two-literal-character floor exactly as a name term's text
    /// does, so <c>content:ab</c> is a search and <c>content:a</c> is not. The grammar additionally
    /// refuses a one-character needle outright, because summing across the query would otherwise
    /// let <c>content:a report</c> through and grep a whole tree for "a".
    /// </summary>
    public override int LiteralChars => Needle.Length;

    /// <summary>
    /// Not a filter that narrows the index — it narrows nothing until a file is opened, so it must
    /// clear the floor on its literal characters like a word rather than standing alone like
    /// <c>ext:jpg</c>.
    /// </summary>
    public override bool HasFilter => false;

    /// <summary>
    /// The one filter that works on a volume the indexer could only build names for: it reads the
    /// disk rather than a column, so there is no missing metadata for it to depend on.
    /// </summary>
    public override bool NeedsMetadata => false;

    public override void CollectContentNeedles(List<string> into) => into.Add(Needle);

    public override bool NeedsContent => true;
}
