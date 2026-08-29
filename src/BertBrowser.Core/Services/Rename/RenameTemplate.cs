namespace BertBrowser.Core.Services.Rename;

/// <summary>What one piece of a parsed template stands for.</summary>
public enum RenamePart
{
    /// <summary>Text to copy out as-is.</summary>
    Literal,

    /// <summary>The whole name after find/replace and the case transform.</summary>
    Name,

    /// <summary>The name without its extension, after find/replace and the case transform.</summary>
    Base,

    /// <summary>The extension, leading dot included; empty for a folder.</summary>
    Extension,

    /// <summary>The containing folder's name — never its path.</summary>
    Parent,

    /// <summary>The counter. <see cref="RenameSegment.Argument"/> holds the padding digits.</summary>
    Counter,

    /// <summary>The last-modified date. <see cref="RenameSegment.Argument"/> holds the format.</summary>
    Modified,
}

/// <param name="Part">What this piece stands for.</param>
/// <param name="Argument">The literal text, or a token's argument; empty when it has none.</param>
public sealed record RenameSegment(RenamePart Part, string Argument);

/// <summary>
/// Parses a name template into segments, once, so the expander does no string scanning per item
/// and the dialog can report an unusable template while the name is still being typed.
/// </summary>
/// <remarks>
/// Token names are matched case-insensitively, the way <see cref="Services.CommandTemplate"/>
/// matches its own — one app should not hold two conventions. <c>{{</c> and <c>}}</c> are literal
/// braces, which is the only reason a strict reading of a lone brace is affordable: '{' is a legal
/// filename character, so a template that wants one has to be able to say so.
/// </remarks>
public static class RenameTemplate
{
    /// <summary>Parses <paramref name="template"/>, or explains why it cannot be used.</summary>
    public static IReadOnlyList<RenameSegment>? Parse(string template, out string? problem)
    {
        var segments = new List<RenameSegment>();
        var literal = new System.Text.StringBuilder();
        problem = null;

        for (var i = 0; i < template.Length; i++)
        {
            var c = template[i];

            if (c == '}')
            {
                // Doubled is a literal brace; alone it is a closer with nothing to close.
                if (i + 1 < template.Length && template[i + 1] == '}') { literal.Append('}'); i++; continue; }
                problem = "A name template can't contain a lone '}' — write '}}' for a literal brace.";
                return null;
            }

            if (c != '{') { literal.Append(c); continue; }

            if (i + 1 < template.Length && template[i + 1] == '{') { literal.Append('{'); i++; continue; }

            var close = template.IndexOf('}', i + 1);
            if (close < 0)
            {
                problem = "A name template can't contain a lone '{' — write '{{' for a literal brace.";
                return null;
            }

            if (literal.Length > 0) { segments.Add(new RenameSegment(RenamePart.Literal, literal.ToString())); literal.Clear(); }

            var body = template[(i + 1)..close];
            if (Token(body, out var segment, out problem) is false) return null;
            segments.Add(segment!);
            i = close;
        }

        if (literal.Length > 0) segments.Add(new RenameSegment(RenamePart.Literal, literal.ToString()));
        return segments;
    }

    /// <summary>True when <paramref name="template"/> holds at least one token.</summary>
    public static bool HasToken(string template)
    {
        var segments = Parse(template, out _);
        return segments is not null && segments.Any(s => s.Part != RenamePart.Literal);
    }

    /// <summary>True when the parsed template uses any of <paramref name="parts"/>.</summary>
    public static bool Uses(IReadOnlyList<RenameSegment> segments, params RenamePart[] parts) =>
        segments.Any(s => parts.Contains(s.Part));

    private static bool Token(string body, out RenameSegment? segment, out string? problem)
    {
        segment = null;
        problem = null;

        var colon = body.IndexOf(':');
        var name = (colon < 0 ? body : body[..colon]).Trim();
        var argument = colon < 0 ? "" : body[(colon + 1)..];

        var part = name.ToLowerInvariant() switch
        {
            "name" => RenamePart.Name,
            "base" => RenamePart.Base,
            "ext" => RenamePart.Extension,
            "parent" => RenamePart.Parent,
            "n" => RenamePart.Counter,
            "modified" => RenamePart.Modified,
            _ => (RenamePart?)null,
        };

        if (part is null)
        {
            problem = $"'{{{body}}}' is not a name template token. Try {{name}}, {{base}}, {{ext}}, " +
                "{parent}, {n} or {modified} — or write '{{' for a literal brace.";
            return false;
        }

        switch (part)
        {
            case RenamePart.Counter when argument.Length > 0 && !argument.All(char.IsAsciiDigit):
                problem = $"'{{{body}}}' is not a counter width — write {{n:000}} to pad to three digits.";
                return false;

            case RenamePart.Modified:
                break;

            case RenamePart.Counter:
                break;

            default:
                if (argument.Length > 0)
                {
                    problem = $"'{{{name}}}' does not take a ':' argument.";
                    return false;
                }
                break;
        }

        segment = new RenameSegment(part.Value, argument);
        return true;
    }
}
