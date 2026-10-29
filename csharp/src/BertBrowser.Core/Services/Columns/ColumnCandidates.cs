namespace BertBrowser.Core.Services.Columns;

/// <summary>Which of the add popup's two lists a candidate belongs to.</summary>
public enum ColumnCandidateKind
{
    /// <summary>The built-ins and <see cref="ColumnCatalog.Curated"/> — what nearly everyone wants,
    /// answerable without reading the property system at all.</summary>
    Common,

    /// <summary>Everything else this machine reported.</summary>
    All,
}

/// <param name="Header">The label to show: the shell's localised name where there is one, and the
/// catalogue's fallback where there is not.</param>
/// <param name="Detail">The canonical name, shown dimmed beside the label, or empty for a built-in.
/// It is what lands in settings.json and the only way to tell two similarly-named properties
/// apart.</param>
public sealed record ColumnCandidate(string Id, string Header, string Detail, ColumnKind Kind);

/// <param name="Title">The section heading. A group with nothing in it is dropped rather than
/// rendered as a bare heading.</param>
public sealed record ColumnCandidateGroup(
    ColumnCandidateKind Kind, string Title, IReadOnlyList<ColumnCandidate> Items);

/// <summary>
/// What the "Add column" popup shows: the columns that could be added, grouped and filtered.
/// </summary>
/// <remarks>
/// <para>
/// Pure, so the one hard part of that popup — what is curated, what has already been added, what the
/// machine reported and what the search matched — is settled where xUnit can reach it rather than
/// inside a <c>TextChanged</c> handler.
/// </para>
/// <para>
/// The popup only ever <em>adds</em>. Removing is the list's own job (its per-row ×) and the header
/// menu's checkmarks, which is why anything already in the layout is simply absent here rather than
/// shown ticked.
/// </para>
/// </remarks>
/// <param name="IsLoading">The property system has not been enumerated yet. Common is still worth
/// showing — it needs no machine — so the popup opens filled rather than blank.</param>
/// <param name="IsFull">The layout is already at <see cref="ColumnLayoutRules.MaxColumns"/>, so
/// nothing can be added. Distinct from a search that matched nothing, and says so differently.</param>
public sealed record ColumnCandidates(
    IReadOnlyList<ColumnCandidateGroup> Groups, bool IsLoading, bool IsFull)
{
    public bool IsEmpty => Groups.Count == 0;

    /// <param name="layout">The columns already chosen. Null means never configured, and is read the
    /// same way <see cref="ColumnLayoutRules.Normalize"/> reads it.</param>
    /// <param name="machine">What <c>ShellProperties.EnumerateDescriptions</c> returned — canonical
    /// name and localised display name. Empty while it is still being read.</param>
    /// <param name="propertiesLoaded">False until that enumeration has come back. An empty
    /// <paramref name="machine"/> is otherwise ambiguous between "still reading" and "this PC
    /// reported none", and the two need different words on screen.</param>
    public static ColumnCandidates Build(
        IReadOnlyList<ColumnSetting>? layout,
        IReadOnlyList<(string Canonical, string Display)> machine,
        string search,
        bool propertiesLoaded)
    {
        var current = ColumnLayoutRules.Normalize(layout);

        // Doubles as the not-already-offered set: every list below adds to it as it goes, which is
        // what keeps a curated property from appearing again under All properties.
        var taken = current.Select(c => c.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);
        var full = current.Count >= ColumnLayoutRules.MaxColumns;

        var needle = search.Trim();
        var groups = new List<ColumnCandidateGroup>(2);

        // The shell's own labels beat the synthesized ones ColumnCatalog falls back to: on a German
        // Windows "Aufnahmedatum" is the name of that property and "DateTaken" is not.
        var localised = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (canonical, display) in machine)
            localised.TryAdd(canonical, display);

        if (!full)
        {
            var common = new List<ColumnCandidate>();
            foreach (var spec in ColumnCatalog.BuiltIns.Concat(ColumnCatalog.Curated))
            {
                if (ColumnCatalog.IsInjected(spec.Id) || !taken.Add(spec.Id)) continue;
                common.Add(Candidate(spec, localised));
            }

            Add(ColumnCandidateKind.Common, "Common", common, needle, groups);

            var rest = new List<ColumnCandidate>();
            foreach (var (canonical, display) in machine)
            {
                if (ColumnCatalog.ShadowedByBuiltIn.Contains(canonical)) continue;
                if (!ColumnId.LooksCanonical(canonical)) continue;
                if (!taken.Add(canonical)) continue;
                rest.Add(new ColumnCandidate(canonical, display, canonical, ColumnKind.ShellProperty));
            }

            rest.Sort((a, b) => string.Compare(a.Header, b.Header, StringComparison.CurrentCultureIgnoreCase));
            Add(ColumnCandidateKind.All, "All properties", rest, needle, groups);
        }

        return new ColumnCandidates(groups, IsLoading: !propertiesLoaded && !full, IsFull: full);
    }

    private static ColumnCandidate Candidate(
        ColumnSpec spec, IReadOnlyDictionary<string, string> localised)
    {
        if (spec.Kind == ColumnKind.BuiltIn)
            return new ColumnCandidate(spec.Id, spec.Header, "", spec.Kind);

        var header = localised.TryGetValue(spec.Id, out var display) ? display : spec.Header;
        return new ColumnCandidate(spec.Id, header, spec.Id, spec.Kind);
    }

    /// <summary>Filters a group and adds it, unless the search emptied it — a heading over nothing
    /// reads as a rendering fault.</summary>
    private static void Add(
        ColumnCandidateKind kind,
        string title,
        List<ColumnCandidate> items,
        string needle,
        List<ColumnCandidateGroup> groups)
    {
        var matched = needle.Length == 0 ? items : items.Where(c => Matches(c, needle)).ToList();
        if (matched.Count > 0) groups.Add(new ColumnCandidateGroup(kind, title, matched));
    }

    /// <summary>The canonical name is searchable as well as the label, because it is on the row and
    /// because it is the half a person is likely to have seen in settings.json.</summary>
    private static bool Matches(ColumnCandidate candidate, string needle) =>
        candidate.Header.Contains(needle, StringComparison.CurrentCultureIgnoreCase) ||
        candidate.Detail.Contains(needle, StringComparison.OrdinalIgnoreCase);
}
