using System.Text;

namespace BertBrowser.Core.Services;

/// <summary>
/// A parsed name-search query: whitespace-separated terms, ANDed, each matched as a
/// case-insensitive substring of the entry name. Terms may contain explicit '*'
/// (any run) and '?' (single character) wildcards. Case folding happens in C#
/// (ToUpperInvariant), consistent with PathKey — SQLite never folds case for us.
/// </summary>
public sealed class SearchQuery
{
    /// <summary>Uppercased terms as typed (wildcards preserved).</summary>
    public IReadOnlyList<string> Terms { get; }

    /// <summary>Per-term SQLite GLOB patterns: "*TERM*" with '[' escaped as "[[]".</summary>
    public IReadOnlyList<string> GlobPatterns { get; }

    private readonly string[] _patterns; // "*TERM*" for the in-memory matcher

    private SearchQuery(string[] terms)
    {
        Terms = terms;
        _patterns = terms.Select(t => "*" + t + "*").ToArray();
        GlobPatterns = terms.Select(t => "*" + EscapeGlob(t) + "*").ToArray();
    }

    /// <summary>
    /// Parses user input. Returns null when the text is empty or contains fewer than
    /// two literal (non-wildcard) characters — too broad to be a useful search.
    /// </summary>
    public static SearchQuery? Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return null;

        var terms = text.ToUpperInvariant()
            .Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);

        var literalChars = terms.Sum(t => t.Count(c => c is not ('*' or '?')));
        return literalChars >= 2 ? new SearchQuery(terms) : null;
    }

    /// <summary>True if every term matches the (display-cased) entry name.</summary>
    public bool Matches(string name)
    {
        var upper = name.ToUpperInvariant();
        foreach (var pattern in _patterns)
        {
            if (!WildcardMatch(upper, pattern))
                return false;
        }
        return true;
    }

    /// <summary>
    /// Classic two-pointer wildcard match with star backtracking. No Regex: this runs
    /// once per term for every entry a live scan visits.
    /// </summary>
    private static bool WildcardMatch(ReadOnlySpan<char> s, ReadOnlySpan<char> p)
    {
        int si = 0, pi = 0, star = -1, match = 0;
        while (si < s.Length)
        {
            if (pi < p.Length && (p[pi] == '?' || p[pi] == s[si]))
            {
                si++;
                pi++;
            }
            else if (pi < p.Length && p[pi] == '*')
            {
                star = pi++;
                match = si;
            }
            else if (star >= 0)
            {
                pi = star + 1;
                si = ++match;
            }
            else
            {
                return false;
            }
        }
        while (pi < p.Length && p[pi] == '*')
            pi++;
        return pi == p.Length;
    }

    /// <summary>
    /// '[' opens a character class in GLOB, so it is the one character that needs
    /// escaping ("[[]" is a class matching a literal '['). A bare ']' outside a
    /// class is already literal, and '*'/'?' are our user-facing wildcards.
    /// </summary>
    private static string EscapeGlob(string term)
    {
        if (!term.Contains('['))
            return term;

        var sb = new StringBuilder(term.Length + 4);
        foreach (var c in term)
        {
            if (c == '[')
                sb.Append("[[]");
            else
                sb.Append(c);
        }
        return sb.ToString();
    }
}
