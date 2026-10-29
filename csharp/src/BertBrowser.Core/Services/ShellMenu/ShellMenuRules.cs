namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>
/// The decisions behind hosting other programs' context-menu entries: which registrations become
/// one extension, which are never shown, what the user has hidden, and how the shell's menu text
/// becomes a WPF header.
/// </summary>
/// <remarks>
/// <para>
/// The app composes the menu itself, one contributor at a time, rather than asking the shell for
/// its finished default menu. The finished one mixes in Cut, Copy, Delete, Rename, Properties,
/// Send to and Share — all of which this app already has — and gives no way to tell which handler
/// produced an item. Composing per handler is what makes "hide this one" possible at all, and it is
/// also why no stock verb needs filtering by its (localised) text.
/// </para>
/// <para>
/// Nothing here opens a registry key or a COM object. The App reads raw registrations and hands
/// them in; this decides.
/// </para>
/// </remarks>
public static class ShellMenuRules
{
    /// <summary>
    /// Static verbs never offered, because BertBrowser has its own item for what they do or because
    /// Windows itself supplies them everywhere. <c>edit</c> and <c>print</c> are deliberately
    /// absent: they are a file type's own verbs, Explorer shows them, and the app has nothing for
    /// them.
    /// </summary>
    public static IReadOnlySet<string> HiddenVerbs { get; } = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
    {
        "open", "explore", "opennewwindow", "opennewprocess", "opencontaining", "find",
        "runas", "runasuser", "cmd", "powershell", "pintohome", "pintostartscreen", "printto",
    };

    /// <summary>
    /// Windows' own handlers for things this app already has — Open with, Send to, Share, Give
    /// access to, Restore previous versions, Copy as path, the pin-to and library verbs, Extract
    /// All on a zip. They register under <c>ContextMenuHandlers</c> exactly as 7-Zip does, so
    /// without this list the menu would offer Explorer's stock items twice over.
    /// </summary>
    public static IReadOnlySet<Guid> StockHandlers { get; } = new HashSet<Guid>
    {
        new("09799AFB-AD67-11D1-ABCD-00C04FC30936"), // Open With
        new("7BA4C740-9E81-11CF-99D3-00AA004AE837"), // Send To
        new("F81E9010-6EA4-11CE-A7FF-00AA003CA9F6"), // Sharing ("Give access to")
        new("E2BF9676-5F8F-435C-97EB-11607A5BEDF7"), // Modern Sharing ("Share")
        new("596AB062-B4D2-4215-9F74-E9109B0A8153"), // Previous Versions
        new("F3D06E7C-1E45-4A26-847E-F9FCDEE59BE0"), // Copy As Path
        new("B455F46E-E4AF-4035-B0A4-CF18D2F6F28E"), // Pin to Quick access
        new("470C0EBD-5D73-4D58-9CED-E91E22E23282"), // Pin to Start
        new("90AA3A4E-1CBA-4233-B8BB-535773D48449"), // Pin to taskbar
        new("3DAD6C5D-2167-4CAE-9914-F99E41C12CFA"), // Include in library
        new("7AD84985-87B4-4A16-BE58-8B72A5B390F7"), // Play To ("Cast to Device")
        new("B8CDCB65-B1BF-4B42-9428-1DFDB7EE92AF"), // Compressed Folder ("Extract All…")
        new("BD472F60-27FA-11CF-B8B4-444553540000"), // Compressed Folder, the older registration
        new("D6791A63-E7E2-4FEE-BF52-5DED8E86E9B8"), // Portable Devices
    };

    /// <summary>
    /// Merges raw registrations into extensions: one per CLSID, one per verb name, in the order
    /// Explorer shows them — static verbs first, then handlers, each as first registered.
    /// </summary>
    /// <param name="blocked">CLSIDs Windows itself refuses to load (the <c>Shell Extensions\Blocked</c>
    /// list). Honoured for the same reason Explorer honours it. <see cref="StockHandlers"/> are
    /// left out as well.</param>
    public static IReadOnlyList<ShellExtension> Catalog(
        IEnumerable<ShellHandlerRegistration> handlers,
        IEnumerable<ShellVerbRegistration> verbs,
        IReadOnlySet<Guid> blocked)
    {
        var rows = new List<ShellExtension>();
        var byId = new Dictionary<string, int>(StringComparer.Ordinal);

        foreach (var verb in verbs)
        {
            if (!IsOfferable(verb)) continue;

            var id = ShellExtension.VerbId(verb.Verb);
            if (byId.TryGetValue(id, out var index))
            {
                var existing = rows[index];
                rows[index] = existing with
                {
                    // A later family may be the one that carries the display name.
                    Name = existing.Name.Length > 0 || verb.DisplayName is null ? existing.Name : verb.DisplayName,
                    Verbs = [.. existing.Verbs, verb],
                    Families = Append(existing.Families, verb.Family),
                };
                continue;
            }

            byId[id] = rows.Count;
            rows.Add(new ShellExtension(
                id, VerbName(verb), ShellExtensionKind.Verb, null, [verb], [verb.Family]));
        }

        foreach (var handler in handlers)
        {
            if (blocked.Contains(handler.Clsid) || StockHandlers.Contains(handler.Clsid)) continue;

            var id = ShellExtension.HandlerId(handler.Clsid);
            if (byId.TryGetValue(id, out var index))
            {
                var existing = rows[index];
                rows[index] = existing with { Families = Append(existing.Families, handler.Family) };
                continue;
            }

            byId[id] = rows.Count;
            rows.Add(new ShellExtension(
                id, HandlerName(handler), ShellExtensionKind.Handler, handler.Clsid, [], [handler.Family]));
        }

        // A verb whose every registration lacked a name falls back to the verb itself, once all of
        // them have been seen — capitalised, since a canonical verb ("print") is written lowercase
        // and Explorer shows it from its own string table.
        for (var i = 0; i < rows.Count; i++)
        {
            if (rows[i].Name.Length == 0) rows[i] = rows[i] with { Name = Capitalize(rows[i].Verbs[0].Verb) };
        }

        return rows;
    }

    /// <summary>The catalog minus the master switch and the user's hidden set.</summary>
    public static IReadOnlyList<ShellExtension> Visible(
        IReadOnlyList<ShellExtension> catalog, bool enabled, IEnumerable<string> hiddenIds)
    {
        if (!enabled) return [];

        var hidden = new HashSet<string>(hiddenIds, StringComparer.OrdinalIgnoreCase);
        return catalog.Where(e => !hidden.Contains(e.Id)).ToList();
    }

    /// <summary>
    /// The hidden list to store after the Settings page is saved. An id hidden earlier but not in
    /// today's catalog stays hidden — an uninstalled extension must not quietly reappear when it
    /// is reinstalled, and the page cannot have shown a row for something it never found.
    /// </summary>
    public static IReadOnlyList<string> HiddenAfterSave(
        IEnumerable<string> previouslyHidden,
        IEnumerable<(string Id, bool IsShown)> rows)
    {
        var rowList = rows.ToList();
        var seen = new HashSet<string>(rowList.Select(r => r.Id), StringComparer.OrdinalIgnoreCase);

        var hidden = new List<string>();
        var taken = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var id in previouslyHidden)
        {
            if (!seen.Contains(id) && taken.Add(id)) hidden.Add(id);
        }

        foreach (var (id, isShown) in rowList)
        {
            if (!isShown && taken.Add(id)) hidden.Add(id);
        }

        return hidden;
    }

    /// <summary>
    /// A Win32 menu string as a WPF header. The shell marks the access key with <c>&amp;</c> and
    /// WPF with <c>_</c>, so each has to be translated and the other escaped; a tab introduces a
    /// shortcut hint that the app's menus never show.
    /// </summary>
    public static string Header(string? win32Text)
    {
        if (string.IsNullOrEmpty(win32Text)) return "";

        var text = win32Text;
        var tab = text.IndexOf('\t');
        if (tab >= 0) text = text[..tab];

        var result = new System.Text.StringBuilder(text.Length + 4);
        for (var i = 0; i < text.Length; i++)
        {
            var c = text[i];
            switch (c)
            {
                case '_':
                    result.Append("__");
                    break;
                case '&' when i + 1 < text.Length && text[i + 1] == '&':
                    result.Append('&');
                    i++;
                    break;
                case '&':
                    result.Append('_');
                    break;
                default:
                    result.Append(c);
                    break;
            }
        }

        return result.ToString();
    }

    /// <summary>
    /// What a handler's menu looks like once the shell's habits are taken out: no separator at
    /// either end or beside another, no submenu with nothing in it, no item with no name — an
    /// owner-drawn item whose text the handler kept to itself has nothing to show.
    /// </summary>
    public static IReadOnlyList<ShellMenuEntry> Tidy(IReadOnlyList<ShellMenuEntry> entries)
    {
        var kept = new List<ShellMenuEntry>();

        foreach (var raw in entries)
        {
            var entry = raw;
            if (entry.IsSeparator)
            {
                if (kept.Count == 0 || kept[^1].IsSeparator) continue;
                kept.Add(entry);
                continue;
            }

            if (entry.HasChildren)
            {
                var children = Tidy(entry.Children);
                if (children.Count == 0) continue;
                entry = entry with { Children = children };
            }
            else if (entry.Command is null)
            {
                // A submenu header whose children were all dropped, or one the handler left empty.
                continue;
            }

            if (entry.Header.Length == 0) continue;
            kept.Add(entry);
        }

        while (kept.Count > 0 && kept[^1].IsSeparator) kept.RemoveAt(kept.Count - 1);
        return kept;
    }

    private static bool IsOfferable(ShellVerbRegistration verb)
    {
        if (verb.Extended || verb.LegacyDisable || verb.ProgrammaticOnly) return false;
        if (verb.HasSubCommands || verb.HasDelegateExecute) return false;
        if (string.IsNullOrWhiteSpace(verb.Command)) return false;
        return !HiddenVerbs.Contains(verb.Verb);
    }

    private static string VerbName(ShellVerbRegistration verb) =>
        verb.DisplayName is { Length: > 0 } name ? name : "";

    private static string Capitalize(string verb) =>
        verb.Length == 0 ? verb : char.ToUpperInvariant(verb[0]) + verb[1..];

    /// <summary>The registration's key name, unless that is just the CLSID written out again, in
    /// which case the class's own registered name is the better label.</summary>
    private static string HandlerName(ShellHandlerRegistration handler)
    {
        if (!Guid.TryParse(handler.Name.Trim(), out _) && handler.Name.Trim().Length > 0)
            return handler.Name.Trim();

        return handler.ClassName is { Length: > 0 } className
            ? className
            : handler.Clsid.ToString("B").ToUpperInvariant();
    }

    private static IReadOnlyList<string> Append(IReadOnlyList<string> families, string family) =>
        families.Contains(family, StringComparer.OrdinalIgnoreCase) ? families : [.. families, family];
}
