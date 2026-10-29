using System.Text;

namespace BertBrowser.Core.Services.Search;

/// <summary>
/// The wildcard vocabulary shared by the in-memory matcher and the SQL it compiles to.
/// Both faces of every text term go through here, so "what does <c>*</c> mean" has one answer.
/// </summary>
internal static class GlobText
{
    /// <summary>
    /// Escapes a run of text for SQLite GLOB. <c>[</c> opens a character class and is the one
    /// character that always needs escaping ("[[]" is a class matching a literal '['); a bare
    /// <c>]</c> outside a class is already literal. When <paramref name="keepWildcards"/> is
    /// false — a quoted phrase, where the user asked for these characters literally —
    /// <c>*</c> and <c>?</c> are escaped as classes too.
    /// </summary>
    public static string Escape(string term, bool keepWildcards)
    {
        var needsWork = term.Contains('[')
            || (!keepWildcards && (term.Contains('*') || term.Contains('?')));
        if (!needsWork) return term;

        var sb = new StringBuilder(term.Length + 8);
        foreach (var c in term)
        {
            if (c == '[') sb.Append("[[]");
            else if (!keepWildcards && (c == '*' || c == '?')) sb.Append('[').Append(c).Append(']');
            else sb.Append(c);
        }
        return sb.ToString();
    }

    /// <summary>
    /// Classic two-pointer wildcard match with star backtracking. No Regex: this runs once per
    /// term for every entry a live scan visits.
    /// </summary>
    public static bool WildcardMatch(ReadOnlySpan<char> s, ReadOnlySpan<char> p)
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
}
