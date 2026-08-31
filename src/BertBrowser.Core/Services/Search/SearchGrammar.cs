using System.Text;
using System.Text.RegularExpressions;

namespace BertBrowser.Core.Services.Search;

/// <summary>The outcome of parsing a query box.</summary>
/// <param name="Query">
/// The runnable query, or null when there is nothing to run — an empty box, or text too broad
/// to be a useful search. Null with a null <paramref name="Problem"/> means "not a search",
/// and the caller shows the plain directory listing.
/// </param>
/// <param name="Problem">
/// Why the text cannot be used, in words for the user. Null when there is nothing wrong.
/// A problem is <em>not</em> the same as "not a search": the caller stays in search mode and
/// shows the message, rather than flipping back to the folder listing mid-keystroke.
/// </param>
public readonly record struct SearchQueryParse(SearchQuery? Query, string? Problem);

/// <summary>
/// Turns query text into a <see cref="SearchNode"/> tree.
/// </summary>
/// <remarks>
/// <para><strong>Never throws.</strong> Every failure — a regular expression that will not
/// compile, an unparseable size, a half-typed <c>size:&gt;</c> — comes back as
/// <see cref="SearchQueryParse.Problem"/> text. This is <c>RenamePattern.ValidateRule</c>'s
/// contract and it is load-bearing for the same reason: this runs on the UI thread on a
/// keystroke, behind a 200 ms debounce.</para>
/// <para><strong>Total, and biased towards a wider result.</strong> Where the input is
/// ambiguous rather than wrong, the grammar degrades to treating text literally instead of
/// refusing: a trailing <c>!</c>, an unbalanced <c>)</c>, an unclosed <c>(</c> and an
/// unrecognised <c>key:</c> are all ordinary text. A search box that says "no" to something a
/// filename could contain is worse than one that searches for it.</para>
/// </remarks>
public static class SearchGrammar
{
    /// <summary>The budget a user-supplied regular expression gets per name, matching
    /// the rename engine's. <c>(a+)+$</c> is three keystrokes away in any search box.</summary>
    private static readonly TimeSpan RegexBudget = TimeSpan.FromMilliseconds(250);

    public static SearchQueryParse Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return new SearchQueryParse(null, null);

        List<Token> tokens;
        try
        {
            tokens = Lex(text);
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            // Defence in depth: the lexer is written not to throw, but this method's whole
            // contract is that a keystroke cannot take the UI thread down.
            return new SearchQueryParse(null, "That search can't be read.");
        }

        if (tokens.Count == 0)
            return new SearchQueryParse(null, null);

        var parser = new Parser(tokens);
        var root = parser.ParseOr();

        if (parser.Problem is not null)
            return new SearchQueryParse(null, parser.Problem);
        if (root is null)
            return new SearchQueryParse(null, null);

        // The floor: something has to narrow the disk down. Two literal characters of text —
        // summed across the query, so "a b" clears it exactly as it always did — or a filter
        // specific enough to stand alone. A bare "a" clears neither, and nor does "is:dir".
        if (root.LiteralChars < 2 && !root.HasFilter)
            return new SearchQueryParse(null, null);

        return new SearchQueryParse(new SearchQuery(root, text), null);
    }

    // ---------------- Lexing ----------------

    private enum TokenKind { Term, Or, Not, Open, Close }

    private readonly record struct Token(TokenKind Kind, string Text, bool Quoted, int ColonIndex);

    /// <summary>
    /// Splits the text into terms and operators. Quotes suppress every special meaning inside
    /// them, which is the documented escape for a filename that really contains a bracket or a
    /// leading exclamation mark.
    /// </summary>
    private static List<Token> Lex(string text)
    {
        var tokens = new List<Token>();
        var i = 0;

        while (i < text.Length)
        {
            if (char.IsWhiteSpace(text[i])) { i++; continue; }

            if (text[i] == '(') { tokens.Add(new Token(TokenKind.Open, "(", false, -1)); i++; continue; }
            if (text[i] == ')') { tokens.Add(new Token(TokenKind.Close, ")", false, -1)); i++; continue; }

            // A '!' only negates something. Trailing, or followed by a space, it is a character
            // in a filename and nothing more.
            if (text[i] == '!' && i + 1 < text.Length && !char.IsWhiteSpace(text[i + 1]))
            {
                tokens.Add(new Token(TokenKind.Not, "!", false, -1));
                i++;
                continue;
            }

            var (token, next) = LexTerm(text, i);
            i = next;

            // Operators are uppercase only: lowercase "or" stays a word, so a file named
            // "Report or Draft" keeps being findable and no query silently changes meaning.
            if (!token.Quoted && token.ColonIndex < 0)
            {
                if (token.Text is "OR" or "|") { tokens.Add(new Token(TokenKind.Or, "OR", false, -1)); continue; }
                if (token.Text is "NOT") { tokens.Add(new Token(TokenKind.Not, "!", false, -1)); continue; }
                if (token.Text is "AND") continue; // adjacency already means AND
            }

            tokens.Add(token);
        }

        return tokens;
    }

    /// <summary>Reads one term, honouring quoted runs, and notes where its first unquoted colon
    /// was so the parser can tell <c>ext:jpg</c> from a name containing a colon.</summary>
    private static (Token Token, int Next) LexTerm(string text, int start)
    {
        var sb = new StringBuilder();
        var quoted = false;
        var colon = -1;
        var i = start;

        while (i < text.Length)
        {
            var c = text[i];
            if (char.IsWhiteSpace(c)) break;

            // Brackets group — except inside a regular expression, where they are the single
            // most common character and structural to the pattern. `re:(a+)+$` has to survive
            // as one token; anything else wanting a literal bracket quotes it.
            if ((c == '(' || c == ')') && !IsRegexValue(sb, colon)) break;

            if (c == '"')
            {
                quoted = true;
                i++;
                while (i < text.Length && text[i] != '"')
                {
                    sb.Append(text[i]);
                    i++;
                }
                // An unclosed quote runs to the end of the text rather than being an error;
                // it is what a half-typed phrase looks like.
                if (i < text.Length) i++;
                continue;
            }

            if (c == ':' && colon < 0) colon = sb.Length;
            sb.Append(c);
            i++;
        }

        return (new Token(TokenKind.Term, sb.ToString(), quoted, colon), i);
    }

    /// <summary>Whether what has been read so far is the <c>re:</c> key and its value.</summary>
    private static bool IsRegexValue(StringBuilder sb, int colon)
    {
        if (colon <= 0) return false;
        var key = sb.ToString(0, colon).ToUpperInvariant();
        return SearchSyntax.Resolve(key) == SearchSyntax.Regex;
    }

    // ---------------- Parsing ----------------

    private sealed class Parser
    {
        private readonly List<Token> _tokens;
        private int _at;

        public Parser(List<Token> tokens) => _tokens = tokens;

        /// <summary>The first thing that made the query unusable, if anything did.</summary>
        public string? Problem { get; private set; }

        private Token? Peek => _at < _tokens.Count ? _tokens[_at] : null;

        /// <summary>OR binds loosest, so it is the outermost level.</summary>
        public SearchNode? ParseOr()
        {
            var branches = new List<SearchNode>();
            var first = ParseAnd();
            if (first is not null) branches.Add(first);

            while (Peek is { Kind: TokenKind.Or })
            {
                _at++;
                var next = ParseAnd();
                if (next is not null) branches.Add(next);
            }

            return branches.Count switch
            {
                0 => null,
                1 => branches[0],
                _ => new OrNode(branches),
            };
        }

        /// <summary>Adjacency is AND — the behaviour a query has always had.</summary>
        private SearchNode? ParseAnd()
        {
            var parts = new List<SearchNode>();

            while (Peek is { } token && token.Kind is not (TokenKind.Or or TokenKind.Close))
            {
                var part = ParseUnary();
                if (part is not null) parts.Add(part);
                if (Problem is not null) return null;
            }

            return parts.Count switch
            {
                0 => null,
                1 => parts[0],
                _ => new AndNode(parts),
            };
        }

        private SearchNode? ParseUnary()
        {
            if (Peek is { Kind: TokenKind.Not })
            {
                _at++;
                var inner = ParseUnary();
                return inner is null ? null : new NotNode(inner);
            }
            return ParsePrimary();
        }

        private SearchNode? ParsePrimary()
        {
            if (Peek is not { } token) return null;

            if (token.Kind == TokenKind.Open)
            {
                _at++;
                var inner = ParseOr();
                // An unclosed '(' closes at the end of the text rather than failing: it is what
                // a group looks like halfway through being typed.
                if (Peek is { Kind: TokenKind.Close }) _at++;
                return inner;
            }

            if (token.Kind == TokenKind.Close)
            {
                // Unbalanced ')' — ordinary text, and a character filenames genuinely contain.
                _at++;
                return new NameTerm(")", literal: true);
            }

            _at++;
            return BuildTerm(token);
        }

        /// <summary>Turns one term token into a node, resolving its key if it has one.</summary>
        private SearchNode? BuildTerm(Token token)
        {
            var text = token.Text;

            // Quoted text is literal throughout: no key, no wildcards.
            if (!token.Quoted && token.ColonIndex > 0)
            {
                var key = text[..token.ColonIndex].ToUpperInvariant();
                var value = text[(token.ColonIndex + 1)..];

                if (SearchSyntax.UnsupportedReason(key) is { } reason)
                {
                    Problem = $"{key.ToLowerInvariant()}: {reason}.";
                    return null;
                }

                if (SearchSyntax.Resolve(key) is { } canonical)
                    return BuildFilter(canonical, key, value, token.Quoted);
            }

            if (text.Length == 0) return null;
            return new NameTerm(text.ToUpperInvariant(), token.Quoted);
        }

        private SearchNode? BuildFilter(string key, string typedKey, string value, bool quoted)
        {
            if (value.Length == 0)
            {
                Problem = $"{typedKey.ToLowerInvariant()}: needs a value.";
                return null;
            }

            switch (key)
            {
                case SearchSyntax.Name:
                    return new NameTerm(value.ToUpperInvariant(), quoted);

                case SearchSyntax.Path:
                    return new PathTerm(value.ToUpperInvariant());

                case SearchSyntax.Extension:
                    var extensions = value
                        .Split(new[] { ';', ',' }, StringSplitOptions.RemoveEmptyEntries)
                        .Select(e => e.TrimStart('.').ToUpperInvariant())
                        .Where(e => e.Length > 0)
                        .ToArray();
                    if (extensions.Length == 0)
                    {
                        Problem = "ext: needs at least one extension, like ext:jpg.";
                        return null;
                    }
                    return new ExtensionTerm(extensions);

                case SearchSyntax.Size:
                    return BuildSize(value);

                case SearchSyntax.Modified:
                    return BuildDate(value);

                case SearchSyntax.Is:
                    return value.ToUpperInvariant() switch
                    {
                        "DIR" or "DIRECTORY" or "FOLDER" => new KindTerm(true),
                        "FILE" => new KindTerm(false),
                        "HIDDEN" => new HiddenTerm(),
                        _ => Fail($"is: doesn't know '{value}' — try is:dir, is:file or is:hidden."),
                    };

                case SearchSyntax.In:
                    return value.ToUpperInvariant() switch
                    {
                        "ARCHIVE" or "ARCHIVES" or "ZIP" or "ZIPS" => new InArchivesTerm(),
                        _ => Fail($"in: doesn't know '{value}' — the only scope is in:archives."),
                    };

                case SearchSyntax.Regex:
                    try
                    {
                        // Interpreted, not RegexOptions.Compiled: this is built on a keystroke
                        // and thrown away on the next one.
                        return new RegexTerm(new Regex(value,
                            RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, RegexBudget));
                    }
                    catch (ArgumentException ex)
                    {
                        return Fail($"That is not a valid regular expression: {ex.Message}");
                    }

                default:
                    return null;
            }
        }

        private SearchNode? BuildSize(string value)
        {
            var (op, rest) = SplitOperator(value);

            // Mid-type: the operator is there and the number is not yet. Say what is missing
            // rather than quoting the empty string back at the user.
            if (rest.Length == 0)
                return Fail("size: needs a number, like size:>100mb.");

            if (rest.Contains("..", StringComparison.Ordinal))
            {
                var parts = rest.Split("..", 2, StringSplitOptions.None);
                if (!SizeText.TryParse(parts[0], out var from) || !SizeText.TryParse(parts[1], out var to))
                    return Fail($"size: can't read the range '{rest}' — try size:1mb..2mb.");
                if (to < from) (from, to) = (to, from);
                return new SizeTerm(from, Bump(to));
            }

            if (rest.Equals("empty", StringComparison.OrdinalIgnoreCase))
                return new SizeTerm(null, 1);

            if (!SizeText.TryParse(rest, out var bytes))
                return Fail($"size: can't read '{rest}' — try size:>100mb.");

            return op switch
            {
                ">" => new SizeTerm(Bump(bytes), null),
                ">=" => new SizeTerm(bytes, null),
                "<" => new SizeTerm(null, bytes),
                "<=" => new SizeTerm(null, Bump(bytes)),
                _ => new SizeTerm(bytes, Bump(bytes)),
            };
        }

        private SearchNode? BuildDate(string value)
        {
            var (op, rest) = SplitOperator(value);
            var now = DateTime.Now;

            if (rest.Length == 0)
                return Fail("dm: needs a date, like dm:today or dm:>2026-01-01.");

            if (rest.Contains("..", StringComparison.Ordinal))
            {
                var parts = rest.Split("..", 2, StringSplitOptions.None);
                if (!DateShorthand.TryResolve(parts[0], now, out var fromLo, out _)
                    || !DateShorthand.TryResolve(parts[1], now, out _, out var toHi))
                    return Fail($"dm: can't read the range '{rest}' — try dm:2026-01-01..2026-06-30.");
                return new DateTerm(fromLo, toHi);
            }

            if (!DateShorthand.TryResolve(rest, now, out var lo, out var hi))
                return Fail($"dm: can't read '{rest}' — try dm:today or dm:2026-08.");

            return op switch
            {
                ">" => new DateTerm(hi, null),
                ">=" => new DateTerm(lo, null),
                "<" => new DateTerm(null, lo),
                "<=" => new DateTerm(null, hi),
                _ => new DateTerm(lo, hi),
            };
        }

        private static (string Op, string Value) SplitOperator(string value)
        {
            if (value.StartsWith(">=", StringComparison.Ordinal)) return (">=", value[2..].Trim());
            if (value.StartsWith("<=", StringComparison.Ordinal)) return ("<=", value[2..].Trim());
            if (value.StartsWith(">", StringComparison.Ordinal)) return (">", value[1..].Trim());
            if (value.StartsWith("<", StringComparison.Ordinal)) return ("<", value[1..].Trim());
            if (value.StartsWith("=", StringComparison.Ordinal)) return ("=", value[1..].Trim());
            return ("", value.Trim());
        }

        /// <summary>The exclusive upper bound one past a value, without overflowing.</summary>
        private static long? Bump(long value) => value == long.MaxValue ? null : value + 1;

        private SearchNode? Fail(string problem)
        {
            Problem ??= problem;
            return null;
        }
    }
}
