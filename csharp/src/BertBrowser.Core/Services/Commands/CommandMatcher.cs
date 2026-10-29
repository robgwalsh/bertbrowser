namespace BertBrowser.Core.Services.Commands;

/// <summary>How well typed text matched a command, strongest first when sorted descending.</summary>
public enum MatchTier
{
    None = 0,

    /// <summary>Found only by its letters in order, by another word for it, by its category or
    /// by its shortcut.</summary>
    Loose = 1,

    /// <summary>A word of the name starts with what was typed, or the name contains it.</summary>
    Word = 2,

    /// <summary>The name starts with what was typed.</summary>
    Start = 3,
}

/// <summary>A run of characters in a name to draw as matched.</summary>
public readonly record struct MatchRange(int Start, int Length);

/// <summary>The verdict on one command for one piece of typed text.</summary>
/// <param name="Score">Higher is better; only meaningful against other scores for the same text.</param>
/// <param name="Highlights">Where in the <em>name</em> the text matched — empty when it matched
/// by an alias, the category or the shortcut, since there is nothing in the name to mark.</param>
public readonly record struct CommandMatch(MatchTier Tier, int Score, IReadOnlyList<MatchRange> Highlights)
{
    public static CommandMatch None { get; } = new(MatchTier.None, 0, []);

    public bool IsMatch => Tier != MatchTier.None;
}

/// <summary>
/// Matches typed text against a command the way a palette has to: forgiving about what was typed,
/// strict about order of preference. Every word typed must be found somewhere — in the name, in
/// another word for the command, in its category or in its shortcut — and where it was found
/// decides the rank, so "new tab" finds New tab before it finds anything that merely contains
/// those letters.
/// </summary>
/// <remarks>
/// The preference, strongest first: the name starts with the word; a word of the name starts with
/// it; the initials of the name's words spell it ("ntb"… "nt" for New tab); the name contains it;
/// an alias matches; the category matches; the letters appear in order. The last is what forgives
/// a typo of omission ("dupl", "prvw") and it is deliberately the weakest, because it also matches
/// things nobody meant.
/// </remarks>
public static class CommandMatcher
{
    public static CommandMatch Match(
        string query, string name, IReadOnlyList<string> aliases, string category, string gesture)
    {
        var words = query.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (words.Length == 0) return CommandMatch.None;

        // A shortcut typed whole — "ctrl+b", "f7" — is a lookup, not a search.
        var typed = string.Concat(words);
        if (gesture.Length > 0 && Same(typed, gesture.Replace(" ", "")))
            return new CommandMatch(MatchTier.Start, 1000, []);

        var tier = MatchTier.Start;
        var score = 0;
        var highlights = new List<MatchRange>();

        foreach (var word in words)
        {
            var best = InName(word, name, highlights);
            if (!best.IsMatch) best = Elsewhere(word, aliases, category, gesture);
            if (!best.IsMatch) best = Subsequence(word, name, highlights);
            if (!best.IsMatch) return CommandMatch.None;

            if (best.Tier < tier) tier = best.Tier;
            score += best.Score;
        }

        // A shorter name is the closer match for the same words: "Copy" before "Copy as path".
        score -= Math.Min(name.Length, 40) / 4;
        return new CommandMatch(tier, score, Merge(highlights));
    }

    private static CommandMatch InName(string word, string name, List<MatchRange> highlights)
    {
        if (name.StartsWith(word, StringComparison.OrdinalIgnoreCase))
        {
            highlights.Add(new MatchRange(0, word.Length));
            return new CommandMatch(MatchTier.Start, 100, []);
        }

        foreach (var start in WordStarts(name))
        {
            if (start == 0 || string.Compare(name, start, word, 0, word.Length, StringComparison.OrdinalIgnoreCase) != 0)
                continue;
            highlights.Add(new MatchRange(start, word.Length));
            return new CommandMatch(MatchTier.Word, 80, []);
        }

        if (word.Length >= 2 && Initials(word, name, highlights))
            return new CommandMatch(MatchTier.Word, 70, []);

        var at = name.IndexOf(word, StringComparison.OrdinalIgnoreCase);
        if (at < 0) return CommandMatch.None;

        highlights.Add(new MatchRange(at, word.Length));
        return new CommandMatch(MatchTier.Word, 60, []);
    }

    private static CommandMatch Elsewhere(
        string word, IReadOnlyList<string> aliases, string category, string gesture)
    {
        foreach (var alias in aliases)
        {
            if (alias.StartsWith(word, StringComparison.OrdinalIgnoreCase))
                return new CommandMatch(MatchTier.Loose, 50, []);
        }

        foreach (var alias in aliases)
        {
            if (alias.Contains(word, StringComparison.OrdinalIgnoreCase))
                return new CommandMatch(MatchTier.Loose, 40, []);
        }

        if (category.StartsWith(word, StringComparison.OrdinalIgnoreCase))
            return new CommandMatch(MatchTier.Loose, 35, []);

        // Part of a shortcut: "ctrl+shift" lists everything on that pair of modifiers.
        if (word.Contains('+') && gesture.Contains(word, StringComparison.OrdinalIgnoreCase))
            return new CommandMatch(MatchTier.Loose, 30, []);

        return CommandMatch.None;
    }

    /// <summary>The word's letters, in order, anywhere in the name. Needs three letters: with two,
    /// nearly every pair of letters is "in order" in something.</summary>
    private static CommandMatch Subsequence(string word, string name, List<MatchRange> highlights)
    {
        if (word.Length < 3) return CommandMatch.None;

        // Tried from every place the first letter occurs, keeping the tightest run: matching
        // leftmost-first would find the "p" of "Copy" for "pth" and mark half the name, when the
        // letters are sitting together in "path".
        List<int>? best = null;
        for (var from = IndexOf(name, word[0], 0); from >= 0; from = IndexOf(name, word[0], from + 1))
        {
            var run = new List<int> { from };
            var at = from + 1;
            foreach (var letter in word.Skip(1))
            {
                var next = IndexOf(name, letter, at);
                if (next < 0) { run = null; break; }
                run.Add(next);
                at = next + 1;
            }

            // Later starts only have less of the name left, so the first failure is the last try.
            if (run is null) break;
            if (best is null || run[^1] - run[0] < best[^1] - best[0]) best = run;
        }

        if (best is null) return CommandMatch.None;

        // The tighter the letters sit together, the likelier this is what was meant.
        var spread = best[^1] - best[0] + 1 - word.Length;
        highlights.AddRange(best.Select(i => new MatchRange(i, 1)));
        return new CommandMatch(MatchTier.Loose, Math.Max(5, 30 - spread), []);
    }

    /// <summary>"nt" for New tab, "cap" for Copy as path: one letter per word, from the first.</summary>
    private static bool Initials(string word, string name, List<MatchRange> highlights)
    {
        var starts = WordStarts(name).ToList();
        if (starts.Count < word.Length) return false;

        for (var i = 0; i < word.Length; i++)
        {
            if (char.ToUpperInvariant(name[starts[i]]) != char.ToUpperInvariant(word[i])) return false;
        }

        for (var i = 0; i < word.Length; i++) highlights.Add(new MatchRange(starts[i], 1));
        return true;
    }

    private static IEnumerable<int> WordStarts(string name)
    {
        for (var i = 0; i < name.Length; i++)
        {
            if (char.IsLetterOrDigit(name[i]) && (i == 0 || !char.IsLetterOrDigit(name[i - 1])))
                yield return i;
        }
    }

    private static int IndexOf(string text, char letter, int from)
    {
        for (var i = from; i < text.Length; i++)
        {
            if (char.ToUpperInvariant(text[i]) == char.ToUpperInvariant(letter)) return i;
        }
        return -1;
    }

    private static bool Same(string a, string b) => a.Equals(b, StringComparison.OrdinalIgnoreCase);

    /// <summary>Sorted, with touching and overlapping runs joined, so the view draws each stretch once.</summary>
    private static IReadOnlyList<MatchRange> Merge(List<MatchRange> ranges)
    {
        if (ranges.Count < 2) return ranges;

        var merged = new List<MatchRange>();
        foreach (var range in ranges.OrderBy(r => r.Start))
        {
            if (merged.Count > 0 && merged[^1].Start + merged[^1].Length >= range.Start)
            {
                var last = merged[^1];
                var end = Math.Max(last.Start + last.Length, range.Start + range.Length);
                merged[^1] = new MatchRange(last.Start, end - last.Start);
            }
            else
            {
                merged.Add(range);
            }
        }

        return merged;
    }
}
