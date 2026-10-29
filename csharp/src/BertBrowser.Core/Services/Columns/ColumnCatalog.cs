using BertBrowser.Core.Services.Preview;

namespace BertBrowser.Core.Services.Columns;

/// <summary>
/// Every column this app can show: the built-ins it answers itself, and the curated shell properties
/// it offers on the header menu. One place, so the menu, the picker dialog, the settings page and the
/// harness cannot disagree about what is on offer — the reason <c>ResolvedNewFileTypes</c> exists.
/// </summary>
public static class ColumnCatalog
{
    // --- Built-in ids. These strings are load-bearing twice over: they are the header Tags the
    // click handler reads, and they are the values already written into SessionTab.SortBy by every
    // build so far. Renaming one silently retires a saved sort order.
    public const string Name = "Name";
    public const string Size = "Size";
    public const string Type = "Type";
    public const string Modified = "Modified";
    public const string Created = "Created";
    public const string Accessed = "Accessed";
    public const string Attributes = "Attributes";
    public const string Extension = "Extension";
    public const string RelativePath = "RelativePath";
    public const string Match = "Match";

    /// <summary>How this row stands against the other pane, while a comparison is running. Injected
    /// because colour alone is not something every reader can use, and because the tints are
    /// necessarily faint — no contrast test can check that five washes are told apart.</summary>
    public const string CompareStatus = "CompareStatus";

    /// <summary>Columns the app places itself, following the list's mode rather than anyone's
    /// choice. They are never persisted and never appear on the header menu.</summary>
    public static bool IsInjected(string id) =>
        id is RelativePath or Match or CompareStatus;

    public static IReadOnlyList<ColumnSpec> BuiltIns { get; } =
    [
        new(Name, "Name", ColumnKind.BuiltIn, 320),
        new(RelativePath, "Folder", ColumnKind.BuiltIn, 220),
        new(Match, "Match", ColumnKind.BuiltIn, 420, Sortable: false),
        new(CompareStatus, "Status", ColumnKind.BuiltIn, 110, Sortable: false),
        new(Size, "Size", ColumnKind.BuiltIn, 110, RightAligned: true),
        new(Type, "Type", ColumnKind.BuiltIn, 120),
        new(Modified, "Modified", ColumnKind.BuiltIn, 140),
        new(Created, "Created", ColumnKind.BuiltIn, 140),
        new(Accessed, "Accessed", ColumnKind.BuiltIn, 140),
        new(Attributes, "Attributes", ColumnKind.BuiltIn, 90),
        new(Extension, "Extension", ColumnKind.BuiltIn, 90),
    ];

    /// <summary>
    /// Shell properties that would sit beside a built-in saying nearly the same thing.
    /// </summary>
    /// <remarks>
    /// Two Type columns — the built-in's "PNG file" and <c>System.ItemTypeText</c>'s "PNG image" —
    /// is worse than one, and the shell one is blank until it hydrates while the free one never is.
    /// The picker dialog enumerates the whole property system, so this is the one place that
    /// exclusion can be stated.
    /// </remarks>
    public static IReadOnlySet<string> ShadowedByBuiltIn { get; } =
        new HashSet<string>(StringComparer.OrdinalIgnoreCase)
        {
            "System.ItemNameDisplay",
            "System.ItemTypeText",
            "System.Size",
            "System.DateModified",
            "System.DateCreated",
            "System.DateAccessed",
            "System.FileAttributes",
            "System.FileExtension",
            "System.ItemFolderPathDisplay",
        };

    /// <summary>The shell properties offered directly on the header menu, grouped. Everything else
    /// on the machine is reachable through "More columns…".</summary>
    public static IReadOnlyList<ColumnSpec> Curated { get; } = BuildCurated();

    private static IReadOnlyList<ColumnSpec> BuildCurated()
    {
        var specs = new List<ColumnSpec>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        // Composed from the preview pane's own lists rather than retyped: 38 canonical strings that
        // would otherwise drift with nothing to notice. Media and Document share System.Title.
        Add(PreviewMetadata.ImageOrder, "Image");
        Add(PreviewMetadata.MediaOrder, "Media");
        Add(PreviewMetadata.DocumentOrder, "Document");
        Add(["System.FileOwner", "System.Keywords", "System.Rating", "System.Comment"], "File");

        return specs;

        void Add(IReadOnlyList<string> canonicals, string group)
        {
            foreach (var canonical in canonicals)
            {
                if (ShadowedByBuiltIn.Contains(canonical) || !seen.Add(canonical)) continue;
                specs.Add(SpecForProperty(canonical, group));
            }
        }
    }

    /// <summary>A spec for a canonical name, curated or not. The header is the tail of the key; the
    /// real localised one replaces it when the column is added.</summary>
    public static ColumnSpec SpecForProperty(string canonical, string group = "") =>
        new(canonical, PreviewMetadata.ShortenCanonical(canonical), ColumnKind.ShellProperty, 140, Group: group);

    /// <summary>Today's columns, at today's widths — what a profile that has never configured them
    /// shows.</summary>
    public static IReadOnlyList<ColumnSetting> Defaults() =>
    [
        new(Name, 320),
        new(Size, 110),
        new(Type, 120),
        new(Modified, 140),
    ];

    private static readonly Dictionary<string, ColumnSpec> ById =
        BuiltIns.ToDictionary(s => s.Id, StringComparer.OrdinalIgnoreCase);

    /// <summary>The spec for an id, or null when this build cannot render it.</summary>
    /// <remarks>
    /// An unknown id that <see cref="ColumnId.LooksCanonical"/> accepts is a property this machine
    /// may simply have no handler for, so it gets a synthesized spec and renders blank. An unknown
    /// bare word came from a newer build and names a column this one does not have, so it is null and
    /// the layout drops it.
    /// </remarks>
    public static ColumnSpec? TryGet(string? id)
    {
        if (string.IsNullOrEmpty(id)) return null;
        if (ById.TryGetValue(id, out var spec)) return spec;
        if (!ColumnId.LooksCanonical(id)) return null;
        return CuratedById.TryGetValue(id, out var curated) ? curated : SpecForProperty(id);
    }

    private static readonly Dictionary<string, ColumnSpec> CuratedById =
        Curated.ToDictionary(s => s.Id, StringComparer.OrdinalIgnoreCase);

    /// <summary>The spec to sort by, degrading to Name rather than letting an unusable id reach the
    /// comparer. <c>SessionTab.SortBy</c>'s "degrade, never fail" rule, one level down.</summary>
    public static ColumnSpec SortSpec(string? id) =>
        TryGet(id) is { Sortable: true } spec ? spec : ById[Name];
}
