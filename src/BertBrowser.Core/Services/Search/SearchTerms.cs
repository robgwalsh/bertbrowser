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

    public override bool Matches(in SearchCandidate candidate) =>
        Literal
            ? candidate.NameKey.Contains(Term, StringComparison.Ordinal)
            : GlobText.WildcardMatch(candidate.NameKey, _pattern);

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

    public override bool Matches(in SearchCandidate candidate) =>
        GlobText.WildcardMatch(candidate.PathKey, _pattern);

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

    public override bool Matches(in SearchCandidate candidate)
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
                return true;
        }
        return false;
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

    public override bool Matches(in SearchCandidate candidate)
    {
        if (candidate.IsDirectory) return false;
        if (Lo is { } lo && candidate.SizeBytes < lo) return false;
        if (Hi is { } hi && candidate.SizeBytes >= hi) return false;
        return true;
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

    public override bool Matches(in SearchCandidate candidate)
    {
        if (candidate.ModifiedUtc < Epoch) return false;
        if (Lo is { } lo && candidate.ModifiedUtc < lo) return false;
        if (Hi is { } hi && candidate.ModifiedUtc >= hi) return false;
        return true;
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

    public override bool Matches(in SearchCandidate candidate) => candidate.IsDirectory == IsDirectory;

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
    public override bool Matches(in SearchCandidate candidate) => candidate.Hidden;

    public override void WriteSql(SqlPredicateBuilder builder) => builder.Append("hidden = 1");

    public override bool SqlComplete => true;
    public override int LiteralChars => 0;
    public override bool HasFilter => false;
    public override bool NeedsMetadata => false;

    /// <summary>Search otherwise excludes hidden entries outright, so without this the term
    /// would be filtered away by the caller and read as a broken feature.</summary>
    public override bool WantsHidden => true;
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

    public override bool Matches(in SearchCandidate candidate)
    {
        try
        {
            return _regex.IsMatch(candidate.NameKey);
        }
        catch (RegexMatchTimeoutException)
        {
            // A pattern that blows its budget on one name must not abort the whole search.
            return false;
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
