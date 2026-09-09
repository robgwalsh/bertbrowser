namespace BertBrowser.Core.Services.Diff;

/// <summary>
/// Comparing two texts line by line.
/// </summary>
/// <remarks>
/// <para>
/// Myers' O(ND) algorithm, with the two standard reductions: the common prefix and suffix are
/// trimmed before it starts, and lines are reduced to integer keys so the inner loop never touches
/// a string. Not patience diff — its advantage is readability on moved code, but it needs
/// unique-line anchoring <em>plus</em> a Myers fallback anyway, which is twice the code for a
/// better answer on a minority of cases.
/// </para>
/// <para>
/// <strong>D is bounded, and the fallback is honest.</strong> Myers costs O(ND), and D is the size
/// of the edit script — small for two versions of the same file, which is the case it is for, and
/// as large as both files put together for two files that share nothing. Past the budget it stops
/// and returns a plain positional pairing with <see cref="TextDiff.BudgetExceeded"/> set, so the
/// window can say "too different to align" rather than hanging. A diff that admits defeat is better
/// than one that stops responding.
/// </para>
/// </remarks>
public static class TextDiffer
{
    /// <summary>
    /// The furthest apart two texts may be before alignment is abandoned.
    /// </summary>
    /// <remarks>
    /// The backtrack needs a snapshot of the frontier per step, which costs about (D+1)² integers —
    /// roughly 16 MB here. Two versions of one file are a handful of edits apart and never come
    /// near it; two unrelated files exceed it immediately, and for those a line diff was never going
    /// to say anything anyway.
    /// </remarks>
    public const int MaxEditDistance = 2048;

    /// <summary>Lines beyond this on either side are not aligned. A file this long is a log.</summary>
    public const int MaxLines = 200_000;

    /// <summary>How many unchanged lines are kept either side of a change when grouping hunks.</summary>
    public const int ContextLines = 3;

    public static TextDiff Compare(IReadOnlyList<string> left, IReadOnlyList<string> right, DiffOptions? options = null)
    {
        options ??= DiffOptions.Default;

        if (left.Count > MaxLines || right.Count > MaxLines)
            return Positional(left.Count, right.Count, budgetExceeded: true);

        // One pool across both sides, or the ids mean nothing: two separate pools would number each
        // side's first distinct line 0, and every line would "match" the line opposite it.
        var pool = new Dictionary<string, int>(StringComparer.Ordinal);
        var a = Keys(left, options, pool);
        var b = Keys(right, options, pool);

        // Trim what matches at both ends. This is what makes a one-line change in a ten-thousand-line
        // file cost almost nothing: everything but the changed region disappears before Myers starts.
        var prefix = 0;
        while (prefix < a.Length && prefix < b.Length && a[prefix] == b[prefix]) prefix++;

        var suffix = 0;
        while (suffix < a.Length - prefix && suffix < b.Length - prefix
               && a[a.Length - 1 - suffix] == b[b.Length - 1 - suffix]) suffix++;

        var middleA = a.AsSpan(prefix, a.Length - prefix - suffix);
        var middleB = b.AsSpan(prefix, b.Length - prefix - suffix);

        var script = Myers(middleA, middleB);
        if (script is null) return Positional(left.Count, right.Count, budgetExceeded: true);

        var rows = new List<DiffRow>(left.Count + right.Count);

        for (var i = 0; i < prefix; i++) rows.Add(new DiffRow(DiffOp.Equal, i + 1, i + 1));

        Pair(script, prefix, rows);

        for (var i = 0; i < suffix; i++)
        {
            var l = a.Length - suffix + i;
            var r = b.Length - suffix + i;
            rows.Add(new DiffRow(DiffOp.Equal, l + 1, r + 1));
        }

        return Finish(rows, left.Count, right.Count, budgetExceeded: false);
    }

    /// <summary>One line reduced to what "the same line" means under these options.</summary>
    private static int[] Keys(IReadOnlyList<string> lines, DiffOptions options, Dictionary<string, int> pool)
    {
        var keys = new int[lines.Count];

        for (var i = 0; i < lines.Count; i++)
        {
            var key = Normalise(lines[i], options);

            // Interned to an int so Myers compares integers. Two lines that normalise the same get
            // the same key, which is how the options cost nothing per comparison.
            if (!pool.TryGetValue(key, out var id))
            {
                id = pool.Count;
                pool[key] = id;
            }

            keys[i] = id;
        }

        return keys;
    }

    /// <remarks>
    /// Applied to the <em>key</em> only. The text a view shows is always the original — a
    /// normalisation that changed what was displayed would be editing the file on screen.
    /// </remarks>
    private static string Normalise(string line, DiffOptions options)
    {
        var text = line;

        if (options.IgnoreAllWhitespace)
        {
            Span<char> buffer = text.Length <= 256 ? stackalloc char[text.Length] : new char[text.Length];
            var length = 0;
            foreach (var c in text)
            {
                if (!char.IsWhiteSpace(c)) buffer[length++] = c;
            }

            text = new string(buffer[..length]);
        }
        else if (options.IgnoreTrailingWhitespace)
        {
            text = text.TrimEnd();
        }

        return options.IgnoreCase ? text.ToLowerInvariant() : text;
    }

    /// <summary>The edit script, or null when the two are further apart than the budget allows.</summary>
    private static List<(DiffOp Op, int A, int B)>? Myers(ReadOnlySpan<int> a, ReadOnlySpan<int> b)
    {
        int n = a.Length, m = b.Length;
        var maxD = Math.Min(MaxEditDistance, n + m);

        var offset = maxD + 1;
        var v = new int[2 * maxD + 3];
        var trace = new List<int[]>(Math.Min(maxD + 1, 64));

        for (var d = 0; d <= maxD; d++)
        {
            trace.Add((int[])v.Clone());

            for (var k = -d; k <= d; k += 2)
            {
                int x;
                if (k == -d || (k != d && v[k - 1 + offset] < v[k + 1 + offset])) x = v[k + 1 + offset];
                else x = v[k - 1 + offset] + 1;

                var y = x - k;
                while (x < n && y < m && a[x] == b[y]) { x++; y++; }

                v[k + offset] = x;

                if (x >= n && y >= m) return Backtrack(trace, a, b, offset);
            }
        }

        return null;
    }

    private static List<(DiffOp Op, int A, int B)> Backtrack(
        List<int[]> trace, ReadOnlySpan<int> a, ReadOnlySpan<int> b, int offset)
    {
        var script = new List<(DiffOp, int, int)>();
        int x = a.Length, y = b.Length;

        for (var d = trace.Count - 1; d >= 0; d--)
        {
            var v = trace[d];
            var k = x - y;

            int previousK;
            if (k == -d || (k != d && v[k - 1 + offset] < v[k + 1 + offset])) previousK = k + 1;
            else previousK = k - 1;

            var previousX = v[previousK + offset];
            var previousY = previousX - previousK;

            while (x > previousX && y > previousY)
            {
                script.Add((DiffOp.Equal, x - 1, y - 1));
                x--;
                y--;
            }

            if (d == 0) break;

            if (x == previousX) script.Add((DiffOp.Insert, -1, --y));
            else script.Add((DiffOp.Delete, --x, -1));
        }

        script.Reverse();
        return script;
    }

    /// <summary>
    /// Folds a run of deletions beside a run of insertions into paired
    /// <see cref="DiffOp.Replace"/> rows.
    /// </summary>
    /// <remarks>
    /// Without this, three changed lines render as three blank-right rows followed by three
    /// blank-left rows, and the reader has to do the pairing in their head. The remainder of the
    /// longer run stays as plain deletions or insertions, which is honest: four lines becoming two
    /// is two replacements and two deletions, not four of anything.
    /// </remarks>
    private static void Pair(List<(DiffOp Op, int A, int B)> script, int shift, List<DiffRow> rows)
    {
        var deletes = new List<int>();
        var inserts = new List<int>();

        void Flush()
        {
            var count = Math.Max(deletes.Count, inserts.Count);
            for (var i = 0; i < count; i++)
            {
                var left = i < deletes.Count ? deletes[i] + shift + 1 : 0;
                var right = i < inserts.Count ? inserts[i] + shift + 1 : 0;

                var op = left > 0 && right > 0 ? DiffOp.Replace
                    : left > 0 ? DiffOp.Delete
                    : DiffOp.Insert;

                rows.Add(new DiffRow(op, left, right));
            }

            deletes.Clear();
            inserts.Clear();
        }

        foreach (var (op, a, b) in script)
        {
            switch (op)
            {
                case DiffOp.Delete:
                    deletes.Add(a);
                    break;
                case DiffOp.Insert:
                    inserts.Add(b);
                    break;
                default:
                    Flush();
                    rows.Add(new DiffRow(DiffOp.Equal, a + shift + 1, b + shift + 1));
                    break;
            }
        }

        Flush();
    }

    /// <summary>
    /// Line <em>i</em> against line <em>i</em>, for when alignment was abandoned.
    /// </summary>
    /// <remarks>
    /// Never an empty result and never an exception. The window says the two are too different to
    /// align; showing them side by side anyway is still more use than showing nothing, and the
    /// hunks it produces still let prev/next-difference work.
    /// </remarks>
    private static TextDiff Positional(int leftCount, int rightCount, bool budgetExceeded)
    {
        var rows = new List<DiffRow>(Math.Max(leftCount, rightCount));

        for (var i = 0; i < Math.Max(leftCount, rightCount); i++)
        {
            var left = i < leftCount ? i + 1 : 0;
            var right = i < rightCount ? i + 1 : 0;

            var op = left > 0 && right > 0 ? DiffOp.Replace
                : left > 0 ? DiffOp.Delete
                : DiffOp.Insert;

            rows.Add(new DiffRow(op, left, right));
        }

        return Finish(rows, leftCount, rightCount, budgetExceeded);
    }

    private static TextDiff Finish(List<DiffRow> rows, int leftCount, int rightCount, bool budgetExceeded)
    {
        var changed = 0;
        foreach (var row in rows)
        {
            if (row.Op is not DiffOp.Equal) changed++;
        }

        return new TextDiff(rows, Hunks(rows), leftCount, rightCount, changed, budgetExceeded);
    }

    /// <summary>
    /// Groups changed rows into the blocks prev/next-difference steps through.
    /// </summary>
    /// <remarks>
    /// A hunk opens at a change and closes once <see cref="ContextLines"/> unchanged rows have gone
    /// by; two hunks nearer than twice that are merged, because stepping between two changes three
    /// lines apart is a jump that does not move the screen.
    /// </remarks>
    private static IReadOnlyList<DiffHunk> Hunks(List<DiffRow> rows)
    {
        var hunks = new List<DiffHunk>();

        var start = -1;
        var lastChange = -1;

        for (var i = 0; i < rows.Count; i++)
        {
            if (rows[i].Op is DiffOp.Equal) continue;

            if (start < 0)
            {
                start = i;
            }
            else if (i - lastChange > 2 * ContextLines)
            {
                hunks.Add(Make(rows, start, lastChange));
                start = i;
            }

            lastChange = i;
        }

        if (start >= 0) hunks.Add(Make(rows, start, lastChange));

        return hunks;

        static DiffHunk Make(List<DiffRow> rows, int from, int to)
        {
            var row = rows[from];
            return new DiffHunk(from, to - from + 1, row.LeftLine, row.RightLine);
        }
    }
}
