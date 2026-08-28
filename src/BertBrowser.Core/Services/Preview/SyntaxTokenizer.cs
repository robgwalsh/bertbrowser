namespace BertBrowser.Core.Services.Preview;

/// <summary>What one run of characters is, for colouring.</summary>
public enum SyntaxClass
{
    Text,
    Keyword,
    String,
    Comment,
    Number,
    Punctuation,
}

/// <summary>A run of <paramref name="Length"/> characters at <paramref name="Start"/>.</summary>
public readonly record struct SyntaxSpan(int Start, int Length, SyntaxClass Class);

/// <summary>The syntax tables this app knows. <see cref="None"/> is not a failure — it is the
/// answer for plain text, and for any language we have no table for.</summary>
public enum SyntaxLanguage
{
    None,
    CSharp,
    CFamily,
    JavaScript,
    Python,
    PowerShell,
    Shell,
    Sql,
    Css,
    Json,
    Xml,
    Ini,
    Markdown,
}

/// <summary>
/// A deliberately small, hand-rolled tokenizer: enough to make code legible in a preview pane,
/// and nothing more. It produces a flat, ordered, gap-free cover of the text — spans, not a tree —
/// so it has no parser, no error recovery and no opinion about whether the file is valid.
/// </summary>
/// <remarks>
/// Hand-rolled rather than taken from a package because the whole of it is rules, and rules belong
/// in Core where xUnit can reach them. The failure mode that matters is not "the wrong colour": it
/// is a span that overlaps its neighbour or runs past the end of the string, which would throw when
/// the view builds runs from it. <c>SyntaxTokenizerTests</c> asserts the cover property over every
/// language on adversarial input, and <see cref="Merge"/> degrades to plain text rather than
/// returning a cover that does not hold.
/// </remarks>
public static class SyntaxTokenizer
{
    /// <summary>Which table applies to a file name. Extensionless well-known names are matched
    /// whole, because <c>Dockerfile</c> and <c>Makefile</c> have no extension to look at.</summary>
    public static SyntaxLanguage LanguageFor(string name)
    {
        var extension = Path.GetExtension(name);
        if (extension.Length == 0)
            return WholeNames.TryGetValue(Path.GetFileName(name), out var byName) ? byName : SyntaxLanguage.None;

        // ".gitignore" and friends: GetExtension returns the whole name for a leading-dot file,
        // which is exactly the key WholeNames holds.
        if (WholeNames.TryGetValue(extension, out var dotFile))
            return dotFile;

        return ByExtension.TryGetValue(extension, out var language) ? language : SyntaxLanguage.None;
    }

    /// <summary>Tokenizes <paramref name="text"/>. The result covers every character exactly once,
    /// in order, with no gaps and no overlaps — an unknown language yields one
    /// <see cref="SyntaxClass.Text"/> span over the lot.</summary>
    public static IReadOnlyList<SyntaxSpan> Tokenize(string text, SyntaxLanguage language)
    {
        if (text.Length == 0) return [];
        if (language == SyntaxLanguage.None) return [new SyntaxSpan(0, text.Length, SyntaxClass.Text)];

        var spans = language switch
        {
            SyntaxLanguage.Xml => ScanXml(text),
            SyntaxLanguage.Markdown => ScanMarkdown(text),
            _ => ScanGeneric(text, Rules[language]),
        };
        return Merge(spans, text.Length);
    }

    // --- the generic scanner ---

    private sealed record LanguageRules(
        string[] LineComments,
        (string Open, string Close)[] BlockComments,
        char[] StringDelimiters,
        bool BackslashEscapes,
        HashSet<string> Keywords);

    private static List<SyntaxSpan> ScanGeneric(string text, LanguageRules rules)
    {
        var spans = new List<SyntaxSpan>();
        var i = 0;
        while (i < text.Length)
        {
            var start = i;

            if (TryBlockComment(text, ref i, rules)) { spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.Comment)); continue; }
            if (TryLineComment(text, ref i, rules)) { spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.Comment)); continue; }
            if (TryString(text, ref i, rules)) { spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.String)); continue; }

            var c = text[i];
            if (char.IsAsciiDigit(c))
            {
                i++;
                // Letters are consumed too, so 0xFF, 1e9 and 100L each stay one number rather than
                // becoming a number followed by an identifier that might collide with a keyword.
                while (i < text.Length && (char.IsLetterOrDigit(text[i]) || text[i] is '.' or '_')) i++;
                spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.Number));
                continue;
            }

            if (IsIdentifierStart(c))
            {
                i++;
                while (i < text.Length && IsIdentifierPart(text[i])) i++;
                var keyword = rules.Keywords.Contains(text[start..i]);
                spans.Add(new SyntaxSpan(start, i - start, keyword ? SyntaxClass.Keyword : SyntaxClass.Text));
                continue;
            }

            i++;
            spans.Add(new SyntaxSpan(start, 1, IsPunctuation(c) ? SyntaxClass.Punctuation : SyntaxClass.Text));
        }
        return spans;
    }

    private static bool TryBlockComment(string text, ref int i, LanguageRules rules)
    {
        foreach (var (open, close) in rules.BlockComments)
        {
            if (!Matches(text, i, open)) continue;
            var end = text.IndexOf(close, i + open.Length, StringComparison.Ordinal);
            // An unterminated block comment runs to the end of the file, which is what the
            // compiler would say too.
            i = end < 0 ? text.Length : end + close.Length;
            return true;
        }
        return false;
    }

    private static bool TryLineComment(string text, ref int i, LanguageRules rules)
    {
        foreach (var token in rules.LineComments)
        {
            if (!Matches(text, i, token)) continue;
            while (i < text.Length && text[i] is not ('\n' or '\r')) i++;
            return true;
        }
        return false;
    }

    private static bool TryString(string text, ref int i, LanguageRules rules)
    {
        var quote = text[i];
        if (Array.IndexOf(rules.StringDelimiters, quote) < 0) return false;

        i++;
        while (i < text.Length)
        {
            if (rules.BackslashEscapes && text[i] == '\\' && i + 1 < text.Length) { i += 2; continue; }
            if (text[i] == quote) { i++; return true; }
            // None of these languages lets an ordinary string span lines, and allowing it would let
            // one apostrophe swallow the rest of the file as a string.
            if (text[i] == '\n') return true;
            i++;
        }
        return true; // unterminated: to end of file
    }

    // --- XML/HTML, which is shaped nothing like the others ---

    private static List<SyntaxSpan> ScanXml(string text)
    {
        var spans = new List<SyntaxSpan>();
        var i = 0;
        while (i < text.Length)
        {
            var start = i;
            if (Matches(text, i, "<!--"))
            {
                var end = text.IndexOf("-->", i + 4, StringComparison.Ordinal);
                i = end < 0 ? text.Length : end + 3;
                spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.Comment));
                continue;
            }

            if (text[i] == '<')
            {
                i++;
                spans.Add(new SyntaxSpan(start, 1, SyntaxClass.Punctuation));

                // The element name, including a leading '/', '?' or '!'.
                var nameStart = i;
                while (i < text.Length && (IsIdentifierPart(text[i]) || text[i] is '/' or '?' or '!' or ':')) i++;
                if (i > nameStart) spans.Add(new SyntaxSpan(nameStart, i - nameStart, SyntaxClass.Keyword));

                // Attributes, until the tag closes.
                while (i < text.Length && text[i] != '>')
                {
                    var attributeStart = i;
                    if (text[i] is '"' or '\'')
                    {
                        var quote = text[i++];
                        while (i < text.Length && text[i] != quote) i++;
                        if (i < text.Length) i++;
                        spans.Add(new SyntaxSpan(attributeStart, i - attributeStart, SyntaxClass.String));
                        continue;
                    }
                    if (IsIdentifierStart(text[i]))
                    {
                        while (i < text.Length && (IsIdentifierPart(text[i]) || text[i] == ':')) i++;
                        spans.Add(new SyntaxSpan(attributeStart, i - attributeStart, SyntaxClass.Text));
                        continue;
                    }
                    i++;
                    spans.Add(new SyntaxSpan(attributeStart, 1, IsPunctuation(text[attributeStart]) ? SyntaxClass.Punctuation : SyntaxClass.Text));
                }
                if (i < text.Length) { spans.Add(new SyntaxSpan(i, 1, SyntaxClass.Punctuation)); i++; }
                continue;
            }

            // Body text, up to the next tag.
            while (i < text.Length && text[i] != '<') i++;
            spans.Add(new SyntaxSpan(start, i - start, SyntaxClass.Text));
        }
        return spans;
    }

    // --- Markdown: a line-shaped language, so scanned by line ---

    private static List<SyntaxSpan> ScanMarkdown(string text)
    {
        var spans = new List<SyntaxSpan>();
        var i = 0;
        var inFence = false;
        while (i < text.Length)
        {
            var lineStart = i;
            while (i < text.Length && text[i] is not ('\n' or '\r')) i++;
            var lineEnd = i;
            while (i < text.Length && text[i] is '\n' or '\r') i++;

            var line = text[lineStart..lineEnd];
            var trimmed = line.TrimStart();
            var indent = line.Length - trimmed.Length;

            if (trimmed.StartsWith("```", StringComparison.Ordinal) || trimmed.StartsWith("~~~", StringComparison.Ordinal))
            {
                inFence = !inFence;
                spans.Add(new SyntaxSpan(lineStart, i - lineStart, SyntaxClass.Punctuation));
                continue;
            }
            if (inFence) { spans.Add(new SyntaxSpan(lineStart, i - lineStart, SyntaxClass.String)); continue; }

            if (trimmed.StartsWith('#')) { spans.Add(new SyntaxSpan(lineStart, i - lineStart, SyntaxClass.Keyword)); continue; }
            if (trimmed.StartsWith('>')) { spans.Add(new SyntaxSpan(lineStart, i - lineStart, SyntaxClass.Comment)); continue; }

            // A bullet or a numbered marker: colour the marker, leave the text of the item alone.
            var marker = MarkdownMarkerLength(trimmed);
            if (marker > 0)
            {
                if (indent > 0) spans.Add(new SyntaxSpan(lineStart, indent, SyntaxClass.Text));
                spans.Add(new SyntaxSpan(lineStart + indent, marker, SyntaxClass.Punctuation));
                spans.Add(new SyntaxSpan(lineStart + indent + marker, i - lineStart - indent - marker, SyntaxClass.Text));
                continue;
            }

            spans.Add(new SyntaxSpan(lineStart, i - lineStart, SyntaxClass.Text));
        }
        return spans;
    }

    private static int MarkdownMarkerLength(string trimmed)
    {
        if (trimmed.Length >= 2 && trimmed[0] is '-' or '*' or '+' && trimmed[1] == ' ') return 2;
        var digits = 0;
        while (digits < trimmed.Length && char.IsAsciiDigit(trimmed[digits])) digits++;
        if (digits > 0 && digits + 1 < trimmed.Length && trimmed[digits] is '.' or ')' && trimmed[digits + 1] == ' ')
            return digits + 2;
        return 0;
    }

    // --- shared ---

    private static bool Matches(string text, int i, string token) =>
        i + token.Length <= text.Length && string.CompareOrdinal(text, i, token, 0, token.Length) == 0;

    private static bool IsIdentifierStart(char c) => char.IsLetter(c) || c is '_' or '$' or '@';
    private static bool IsIdentifierPart(char c) => char.IsLetterOrDigit(c) || c is '_' or '$' or '-';

    private const char Delete = (char)0x7F;
    private static bool IsPunctuation(char c) => c is > ' ' and < Delete && !char.IsLetterOrDigit(c) && c != '_';

    /// <summary>Collapses adjacent spans of the same class and drops empties, then checks the
    /// result really does cover the text — the invariant the view depends on. A scanner bug
    /// becomes plain text here rather than an exception three layers up.</summary>
    private static IReadOnlyList<SyntaxSpan> Merge(List<SyntaxSpan> spans, int length)
    {
        var merged = new List<SyntaxSpan>(spans.Count);
        foreach (var span in spans)
        {
            if (span.Length <= 0) continue;
            if (merged.Count > 0)
            {
                var last = merged[^1];
                if (last.Class == span.Class && last.Start + last.Length == span.Start)
                {
                    merged[^1] = last with { Length = last.Length + span.Length };
                    continue;
                }
            }
            merged.Add(span);
        }

        var covered = 0;
        foreach (var span in merged)
        {
            if (span.Start != covered || span.Start + span.Length > length)
                return [new SyntaxSpan(0, length, SyntaxClass.Text)];
            covered += span.Length;
        }
        return covered == length ? merged : [new SyntaxSpan(0, length, SyntaxClass.Text)];
    }

    // --- tables ---

    private static HashSet<string> Words(string words) =>
        new(words.Split(' ', StringSplitOptions.RemoveEmptyEntries), StringComparer.Ordinal);

    private static readonly Dictionary<SyntaxLanguage, LanguageRules> Rules = new()
    {
        [SyntaxLanguage.CSharp] = new(
            ["//"], [("/*", "*/")], ['"', '\''], true,
            Words("abstract as async await base bool break byte case catch char checked class const continue decimal default delegate do double dynamic else enum event explicit extern false finally fixed float for foreach get global goto if implicit in init int interface internal is lock long namespace new nint nuint null object operator out override params partial private protected public readonly record ref return sbyte sealed set short sizeof stackalloc static string struct switch this throw true try typeof uint ulong unchecked unsafe ushort using var virtual void volatile when where while with yield")),

        [SyntaxLanguage.CFamily] = new(
            ["//"], [("/*", "*/")], ['"', '\''], true,
            Words("auto bool break case catch char class const constexpr continue default delete do double else enum explicit extern false float for fn friend func goto if impl inline int interface let long mut namespace new nil nullptr operator package private protected pub public range register return self short signed sizeof static struct switch template this throw true try type typedef typename union unsigned use using val var virtual void volatile where while")),

        [SyntaxLanguage.JavaScript] = new(
            ["//"], [("/*", "*/")], ['"', '\'', '`'], true,
            Words("abstract any as async await boolean break case catch class const constructor continue debugger declare default delete do else enum export extends false finally for from function get if implements import in instanceof interface keyof let namespace never new null number of private protected public readonly return set static string super switch symbol this throw true try type typeof undefined unknown var void while with yield")),

        [SyntaxLanguage.Json] = new(
            // JSON proper has no comments, but a .json file in a source tree very often does
            // (tsconfig, launch.json, devcontainer). Colouring them is the useful behaviour.
            ["//"], [("/*", "*/")], ['"'], true,
            Words("true false null")),

        [SyntaxLanguage.Python] = new(
            ["#"], [("\"\"\"", "\"\"\""), ("'''", "'''")], ['"', '\''], true,
            Words("and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return self True try while with yield")),

        [SyntaxLanguage.PowerShell] = new(
            ["#"], [("<#", "#>")], ['"', '\''], false,
            Words("begin break catch class continue data define do dynamicparam else elseif end enum exit filter finally for foreach from function hidden if in param process return static switch throw trap try until using while workflow")),

        [SyntaxLanguage.Shell] = new(
            ["#"], [], ['"', '\''], false,
            Words("if then else elif fi case esac for while until do done in function return break continue local export readonly declare source eval exec set unset shift trap exit echo")),

        [SyntaxLanguage.Sql] = new(
            ["--"], [("/*", "*/")], ['\'', '"'], false,
            Words("ADD ALL ALTER AND AS ASC BEGIN BETWEEN BY CASE CAST COLUMN COMMIT CONSTRAINT CREATE CROSS DECLARE DEFAULT DELETE DESC DISTINCT DROP ELSE END EXEC EXISTS FOREIGN FROM FULL GROUP HAVING IF IN INDEX INNER INSERT INTO IS JOIN KEY LEFT LIKE LIMIT NOT NULL OFFSET ON OR ORDER OUTER PRIMARY REFERENCES RIGHT ROLLBACK SELECT SET TABLE THEN TOP TRANSACTION UNION UNIQUE UPDATE VALUES VIEW WHEN WHERE WITH add all alter and as asc begin between by case cast column commit constraint create cross declare default delete desc distinct drop else end exec exists foreign from full group having if in index inner insert into is join key left like limit not null offset on or order outer primary references right rollback select set table then top transaction union unique update values view when where with")),

        [SyntaxLanguage.Css] = new(
            ["//"], [("/*", "*/")], ['"', '\''], true,
            Words("important media import charset keyframes supports and not only from to")),

        [SyntaxLanguage.Ini] = new(
            ["#", ";"], [], ['"', '\''], false,
            Words("true false yes no on off True False")),
    };

    private static readonly Dictionary<string, SyntaxLanguage> ByExtension = new(StringComparer.OrdinalIgnoreCase)
    {
        [".cs"] = SyntaxLanguage.CSharp, [".csx"] = SyntaxLanguage.CSharp,
        [".c"] = SyntaxLanguage.CFamily, [".h"] = SyntaxLanguage.CFamily, [".cpp"] = SyntaxLanguage.CFamily,
        [".hpp"] = SyntaxLanguage.CFamily, [".cc"] = SyntaxLanguage.CFamily, [".hh"] = SyntaxLanguage.CFamily,
        [".cxx"] = SyntaxLanguage.CFamily, [".mm"] = SyntaxLanguage.CFamily,
        [".java"] = SyntaxLanguage.CFamily, [".kt"] = SyntaxLanguage.CFamily, [".kts"] = SyntaxLanguage.CFamily,
        [".swift"] = SyntaxLanguage.CFamily, [".go"] = SyntaxLanguage.CFamily, [".rs"] = SyntaxLanguage.CFamily,
        [".php"] = SyntaxLanguage.CFamily, [".scala"] = SyntaxLanguage.CFamily, [".dart"] = SyntaxLanguage.CFamily,
        [".groovy"] = SyntaxLanguage.CFamily, [".gradle"] = SyntaxLanguage.CFamily, [".proto"] = SyntaxLanguage.CFamily,
        [".zig"] = SyntaxLanguage.CFamily, [".nim"] = SyntaxLanguage.CFamily,
        [".js"] = SyntaxLanguage.JavaScript, [".mjs"] = SyntaxLanguage.JavaScript, [".cjs"] = SyntaxLanguage.JavaScript,
        [".jsx"] = SyntaxLanguage.JavaScript, [".ts"] = SyntaxLanguage.JavaScript, [".tsx"] = SyntaxLanguage.JavaScript,
        [".json"] = SyntaxLanguage.Json, [".jsonc"] = SyntaxLanguage.Json, [".json5"] = SyntaxLanguage.Json,
        [".webmanifest"] = SyntaxLanguage.Json, [".ipynb"] = SyntaxLanguage.Json,
        [".xml"] = SyntaxLanguage.Xml, [".xaml"] = SyntaxLanguage.Xml, [".html"] = SyntaxLanguage.Xml,
        [".htm"] = SyntaxLanguage.Xml, [".xhtml"] = SyntaxLanguage.Xml, [".svg"] = SyntaxLanguage.Xml,
        [".csproj"] = SyntaxLanguage.Xml, [".fsproj"] = SyntaxLanguage.Xml, [".vbproj"] = SyntaxLanguage.Xml,
        [".props"] = SyntaxLanguage.Xml, [".targets"] = SyntaxLanguage.Xml, [".config"] = SyntaxLanguage.Xml,
        [".resx"] = SyntaxLanguage.Xml, [".nuspec"] = SyntaxLanguage.Xml, [".plist"] = SyntaxLanguage.Xml,
        [".css"] = SyntaxLanguage.Css, [".scss"] = SyntaxLanguage.Css, [".less"] = SyntaxLanguage.Css,
        [".sql"] = SyntaxLanguage.Sql,
        [".py"] = SyntaxLanguage.Python, [".pyw"] = SyntaxLanguage.Python, [".pyi"] = SyntaxLanguage.Python,
        [".ps1"] = SyntaxLanguage.PowerShell, [".psm1"] = SyntaxLanguage.PowerShell, [".psd1"] = SyntaxLanguage.PowerShell,
        [".sh"] = SyntaxLanguage.Shell, [".bash"] = SyntaxLanguage.Shell, [".zsh"] = SyntaxLanguage.Shell,
        [".bat"] = SyntaxLanguage.Shell, [".cmd"] = SyntaxLanguage.Shell,
        [".ini"] = SyntaxLanguage.Ini, [".cfg"] = SyntaxLanguage.Ini, [".conf"] = SyntaxLanguage.Ini,
        [".toml"] = SyntaxLanguage.Ini, [".yml"] = SyntaxLanguage.Ini, [".yaml"] = SyntaxLanguage.Ini,
        [".properties"] = SyntaxLanguage.Ini,
        [".md"] = SyntaxLanguage.Markdown, [".markdown"] = SyntaxLanguage.Markdown, [".mdx"] = SyntaxLanguage.Markdown,
    };

    private static readonly Dictionary<string, SyntaxLanguage> WholeNames = new(StringComparer.OrdinalIgnoreCase)
    {
        ["Dockerfile"] = SyntaxLanguage.Shell,
        ["Makefile"] = SyntaxLanguage.Shell,
        ["Rakefile"] = SyntaxLanguage.Shell,
        [".gitignore"] = SyntaxLanguage.Ini,
        [".gitattributes"] = SyntaxLanguage.Ini,
        [".dockerignore"] = SyntaxLanguage.Ini,
        [".editorconfig"] = SyntaxLanguage.Ini,
        [".npmrc"] = SyntaxLanguage.Ini,
        [".env"] = SyntaxLanguage.Ini,
    };
}
