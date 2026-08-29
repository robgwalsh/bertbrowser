using System.Buffers;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace BertBrowser.Core.Services.Rename;

/// <param name="Name">The name the rule produced.</param>
/// <param name="Problem">Why this one item could not be named properly, or null. Kept per item
/// rather than thrown, because one item's missing date must not cost the other 299 their
/// preview.</param>
public readonly record struct RenamedName(string Name, string? Problem);

/// <summary>
/// Turns what the user asked for into the name each selected item gets. The plain box is one rule —
/// one item takes the typed text as its whole name, several are numbered while keeping their own
/// extensions. The expanded panel adds find/replace, a case transform, a counter and a date,
/// described by <see cref="RenameRule"/>.
/// </summary>
/// <remarks>
/// Pure, and separate from <see cref="RenamePlanner"/>, because the dialog previews the result of
/// every keystroke: the naming rule and the "is this a legal Windows name" rule have to be the same
/// ones the rename itself will use, not a re-implementation that can drift.
///
/// <para><b>Nothing here throws.</b> <see cref="RenamePlanner"/> calls it unguarded and the dialog
/// calls that on the UI thread, so a regular expression that fails to compile, one that backtracks
/// past its deadline, and a date format the framework rejects all come back as text — a
/// <see cref="ValidateRule"/> message, or a per-item <see cref="RenamedName.Problem"/> — rather
/// than as an exception out of a keystroke.</para>
/// </remarks>
public static class RenamePattern
{
    /// <summary>NTFS's per-component limit.</summary>
    public const int MaxNameLength = 255;

    /// <summary>What <c>{modified}</c> means with no format of its own: sortable, and legal.</summary>
    public const string DefaultDateFormat = "yyyy-MM-dd";

    /// <summary>How long one regular expression gets against one name before it is abandoned.
    /// A pattern like <c>(a+)+$</c> is three keystrokes away in any Find box, and this runs on
    /// every one of them, across the whole selection.</summary>
    private static readonly TimeSpan RegexBudget = TimeSpan.FromMilliseconds(250);

    /// <summary>Names Windows still refuses to give a file, with or without an extension.</summary>
    private static readonly HashSet<string> ReservedNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    };

    /// <summary>The names <paramref name="pattern"/> gives <paramref name="sources"/>, one per
    /// source and in the same order. Never throws: an unusable pattern still produces names, and
    /// <see cref="Validate(string)"/> is what rejects them.</summary>
    public static IReadOnlyList<string> Apply(IReadOnlyList<RenameSource> sources, string pattern) =>
        Apply(sources, RenameRule.Simple(pattern)).Select(r => r.Name).ToList();

    /// <summary>The names <paramref name="rule"/> gives <paramref name="sources"/>, one per source
    /// and in the same order, each carrying whatever went wrong for that item alone.</summary>
    public static IReadOnlyList<RenamedName> Apply(
        IReadOnlyList<RenameSource> sources, RenameRule rule)
    {
        if (sources.Count == 0) return [];

        var text = Clean(rule.Template);

        if (rule.IsLiteral) return Literally(sources, text);

        var segments = RenameTemplate.Parse(text, out var problem);
        if (segments is null) return Everything(sources, problem);

        Regex? regex = null;
        if (rule.UseRegex && rule.Find.Length > 0)
        {
            try
            {
                regex = BuildRegex(rule);
            }
            catch (ArgumentException ex)
            {
                return Everything(sources, $"That is not a valid regular expression: {ex.Message}");
            }
        }

        var names = new RenamedName[sources.Count];
        for (var i = 0; i < sources.Count; i++)
            names[i] = One(sources[i], i, rule, segments, regex);
        return names;
    }

    /// <summary>Exactly what a rename has always done with a typed name: one item takes it whole,
    /// several are numbered and keep their own extensions.</summary>
    /// <remarks>
    /// The text is cleaned <em>before</em> the number and extension go on, which is where the
    /// cleaning has always happened and has to stay: doing it afterwards would leave "  Holiday  "
    /// over two files as "Holiday   1.jpg". An empty pattern produces an empty name rather than
    /// " 1.jpg" — a name <see cref="Validate(string)"/> would otherwise accept, from a box the
    /// user had merely cleared.
    /// </remarks>
    private static RenamedName[] Literally(IReadOnlyList<RenameSource> sources, string text)
    {
        var names = new RenamedName[sources.Count];
        if (sources.Count == 1)
        {
            names[0] = new RenamedName(text, null);
            return names;
        }

        for (var i = 0; i < sources.Count; i++)
            names[i] = new RenamedName(
                text.Length == 0 ? "" : text + " " + (i + 1) + Extension(sources[i]), null);
        return names;
    }

    private static RenamedName One(
        RenameSource source, int index, RenameRule rule,
        IReadOnlyList<RenameSegment> segments, Regex? regex)
    {
        var (stem, extension) = Split(source);
        string? problem = null;

        try
        {
            (stem, extension) = Rewrite(stem, extension, source.IsDirectory, rule, regex);
        }
        catch (RegexMatchTimeoutException)
        {
            problem = $"'{source.Name}' took too long to match — try a simpler expression.";
        }

        var built = new StringBuilder();
        foreach (var segment in segments)
        {
            switch (segment.Part)
            {
                case RenamePart.Literal:
                    built.Append(segment.Argument);
                    break;

                case RenamePart.Name:
                    built.Append(stem).Append(extension);
                    break;

                case RenamePart.Base:
                    built.Append(stem);
                    break;

                case RenamePart.Extension:
                    built.Append(extension);
                    break;

                case RenamePart.Parent:
                    built.Append(ParentName(source.Path));
                    break;

                case RenamePart.Counter:
                    // "D3" rather than PadLeft, so a negative step gives -001 and not 0-1.
                    var value = rule.CounterStart + (index * rule.CounterStep);
                    built.Append(value.ToString(
                        segment.Argument.Length > 0 ? "D" + segment.Argument.Length : "D",
                        CultureInfo.InvariantCulture));
                    break;

                case RenamePart.Modified:
                    problem = Date(built, source, segment.Argument) ?? problem;
                    break;
            }
        }

        return new RenamedName(Clean(built.ToString()), problem);
    }

    private static string? Date(StringBuilder built, RenameSource source, string format)
    {
        if (source.Modified is not { } date)
            return $"'{source.Name}' has no modified date to put in its name.";

        try
        {
            built.Append(date.ToString(
                format.Length > 0 ? format : DefaultDateFormat, CultureInfo.InvariantCulture));
            return null;
        }
        catch (FormatException)
        {
            return $"'{format}' is not a date format. Try {DefaultDateFormat}.";
        }
    }

    /// <summary>Applies the find/replace and the case transform to whichever part of the existing
    /// name the rule's scope names, and hands back the two halves the template draws on.</summary>
    private static (string Stem, string Extension) Rewrite(
        string stem, string extension, bool isDirectory, RenameRule rule, Regex? regex)
    {
        switch (rule.Scope)
        {
            case RenameScope.Extension:
                return (stem, Convert(extension, rule, regex));

            case RenameScope.WholeName:
                // Re-split, so {base} and {ext} still mean something after a replace that moved
                // the dot — or removed it.
                return Split(Convert(stem + extension, rule, regex), isDirectory);

            default:
                // A replace routinely leaves a trailing space behind ("report v2" losing "v2"),
                // and the final Clean cannot reach it: by then the name ends in ".txt".
                return (Convert(stem, rule, regex).Trim(), extension);
        }
    }

    private static string Convert(string value, RenameRule rule, Regex? regex)
    {
        if (rule.Find.Length > 0)
            value = regex is not null
                ? regex.Replace(value, rule.Replace)
                : value.Replace(rule.Find, rule.Replace,
                    rule.MatchCase ? StringComparison.Ordinal : StringComparison.OrdinalIgnoreCase);

        return Recase(value, rule.Case);
    }

    /// <summary>Re-cases a name, always invariantly.</summary>
    /// <remarks>
    /// The culture matters more than it looks. Under tr-TR, <c>"FILE".ToLower()</c> is "fıle" with
    /// a dotless ı — a different name, across the whole batch, on somebody else's machine. This is
    /// the discipline <c>PathKey.Canonicalize</c> already keeps, for the same reason.
    ///
    /// <para>Title case lower-cases first because <c>ToTitleCase</c> leaves an already-upper-case
    /// word exactly as it found it, and an all-caps name is the main reason anyone reaches for
    /// it — "HOLIDAY PHOTO" would otherwise come back unchanged and read as a dead button.</para>
    /// </remarks>
    private static string Recase(string value, RenameCase kind) => kind switch
    {
        RenameCase.Lower => value.ToLowerInvariant(),
        RenameCase.Upper => value.ToUpperInvariant(),
        RenameCase.Title => CultureInfo.InvariantCulture.TextInfo.ToTitleCase(value.ToLowerInvariant()),
        RenameCase.Sentence => Sentence(value),
        _ => value,
    };

    private static string Sentence(string value)
    {
        var lowered = value.ToLowerInvariant();
        for (var i = 0; i < lowered.Length; i++)
        {
            if (!char.IsLetter(lowered[i])) continue;
            return string.Concat(
                lowered.AsSpan(0, i),
                char.ToUpperInvariant(lowered[i]).ToString(),
                lowered.AsSpan(i + 1));
        }
        return lowered;
    }

    /// <summary>Why <paramref name="rule"/> cannot be used at all, or null when it can. Separate
    /// from <see cref="Validate(string)"/>, which judges one finished name and is shared with
    /// <c>NewItemPattern</c>: this one never touches name legality.</summary>
    public static string? ValidateRule(RenameRule rule)
    {
        if (rule.IsLiteral) return null;

        if (rule.CounterStep == 0)
            return "Counter step can't be zero — every item would be given the same number.";

        var segments = RenameTemplate.Parse(Clean(rule.Template), out var problem);
        if (segments is null) return problem;

        if (rule.UseRegex && rule.Find.Length > 0)
        {
            try
            {
                _ = BuildRegex(rule);
            }
            catch (ArgumentException ex)
            {
                return $"That is not a valid regular expression: {ex.Message}";
            }
        }

        foreach (var segment in segments)
        {
            if (segment.Part != RenamePart.Modified || segment.Argument.Length == 0) continue;

            string formatted;
            try
            {
                formatted = SampleDate.ToString(segment.Argument, CultureInfo.InvariantCulture);
            }
            catch (FormatException)
            {
                return $"'{segment.Argument}' is not a date format. Try {DefaultDateFormat}.";
            }

            // A standard format such as "d" is perfectly valid and produces slashes; the refusal
            // that followed would name the character rather than the format that put it there.
            if (formatted.AsSpan().IndexOfAny(InvalidChars) >= 0)
                return $"A date written '{segment.Argument}' comes out as '{formatted}', which a " +
                    $"name can't hold. Try {DefaultDateFormat}.";
        }

        return null;
    }

    /// <summary>What the dialog starts with: the one item's whole name, or — for a selection — the
    /// first item's name without its extension, which is the part a numbered rename replaces.</summary>
    public static string SuggestFor(IReadOnlyList<RenameSource> sources)
    {
        if (sources.Count == 0) return "";
        var first = sources[0];
        var name = System.IO.Path.GetFileName(first.Path);
        if (sources.Count == 1) return name;
        return first.IsDirectory ? name : System.IO.Path.GetFileNameWithoutExtension(name);
    }

    /// <summary>The length of the part of a single item's name that a rename usually replaces, so
    /// the dialog can pre-select it and leave the extension alone.</summary>
    public static int BaseNameLength(RenameSource source) => Split(source).Stem.Length;

    /// <summary>Why <paramref name="name"/> can't be a file name, or null when it can.</summary>
    public static string? Validate(string name)
    {
        if (string.IsNullOrWhiteSpace(name))
            return "Enter a name.";

        if (name.Length > MaxNameLength)
            return $"A name can be at most {MaxNameLength} characters.";

        foreach (var c in name)
        {
            if (char.IsControl(c))
                return "A name can't contain control characters.";
            if (InvalidChars.Contains(c))
                return "A name can't contain any of  \\ / : * ? \" < > |";
        }

        if (name[^1] is '.' or ' ')
            return "A name can't end with a space or a period.";

        var dot = name.IndexOf('.');
        var stem = dot < 0 ? name : name[..dot];
        if (ReservedNames.Contains(stem))
            return $"'{stem}' is a name Windows reserves for a device.";

        return null;
    }

    /// <summary>The one place a name is cut into the part before its extension and the extension
    /// itself.</summary>
    /// <remarks>
    /// Two carve-outs, both of which already exist elsewhere and must not be re-decided here. A
    /// folder has no extension, so "My.Project" is not ".Project" over "My". And a dotfile such as
    /// .gitignore is all <em>extension</em> as far as <see cref="System.IO.Path"/> is concerned,
    /// which would leave a find/replace scoped to the name with nothing to work on while an
    /// extension scope rewrote the whole thing — so it is treated as all stem.
    /// </remarks>
    public static (string Stem, string Extension) Split(RenameSource source) =>
        Split(System.IO.Path.GetFileName(source.Path), source.IsDirectory);

    private static (string Stem, string Extension) Split(string name, bool isDirectory)
    {
        if (isDirectory) return (name, "");
        var stem = System.IO.Path.GetFileNameWithoutExtension(name);
        return stem.Length == 0 ? (name, "") : (stem, System.IO.Path.GetExtension(name));
    }

    private static Regex BuildRegex(RenameRule rule) => new(
        rule.Find,
        RegexOptions.CultureInvariant | (rule.MatchCase ? RegexOptions.None : RegexOptions.IgnoreCase),
        RegexBudget);

    /// <summary>Every item left at the name it already has, carrying one rule-level problem — what
    /// a template or an expression that cannot be used at all produces.</summary>
    private static RenamedName[] Everything(IReadOnlyList<RenameSource> sources, string? problem)
    {
        var names = new RenamedName[sources.Count];
        for (var i = 0; i < sources.Count; i++)
            names[i] = new RenamedName(sources[i].Name, problem);
        return names;
    }

    /// <summary>The containing folder's name — "Photos", never "C:\Users\Rob\Photos". A drive root
    /// has no name of its own, so it gives up its letter instead of an empty string.</summary>
    private static string ParentName(string path)
    {
        var parent = System.IO.Path.GetDirectoryName(
            System.IO.Path.TrimEndingDirectorySeparator(path));
        if (string.IsNullOrEmpty(parent)) return "";

        var trimmed = System.IO.Path.TrimEndingDirectorySeparator(parent);
        var name = System.IO.Path.GetFileName(trimmed);
        return name.Length > 0 ? name : trimmed.TrimEnd(':');
    }

    /// <summary>A date that exercises every field of a format string without being ambiguous.</summary>
    private static readonly DateTime SampleDate =
        new(2026, 8, 28, 13, 45, 7, DateTimeKind.Unspecified);

    /// <summary>Cached because <see cref="System.IO.Path.GetInvalidFileNameChars"/> hands out a
    /// fresh copy on every call, and validation runs on every keystroke.</summary>
    private static readonly SearchValues<char> InvalidChars =
        SearchValues.Create(System.IO.Path.GetInvalidFileNameChars());

    /// <summary>Trailing spaces and periods are silently dropped by Windows, so a name that only
    /// differs by them is not a rename at all; take them off before anything sees the name.</summary>
    private static string Clean(string pattern) => pattern.Trim().TrimEnd('.', ' ');

    private static string Extension(RenameSource source) => Split(source).Extension;
}
