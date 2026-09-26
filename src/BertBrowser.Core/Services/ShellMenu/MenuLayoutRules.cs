namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>What one position of an arranged menu holds.</summary>
public enum MenuPlacementKind
{
    BuiltIn,
    Separator,
    Command,
    Shell,
}

/// <summary>One position of a menu as <see cref="MenuLayoutRules.Arrange"/> lays it out. The id is
/// the entry's own (<c>cut</c>, a command's id, <c>clsid:{…}</c>), without its token prefix.</summary>
public readonly record struct MenuPlacement(MenuPlacementKind Kind, string Id)
{
    public static MenuPlacement Separator { get; } = new(MenuPlacementKind.Separator, "");
}

/// <summary>
/// The order of the app's right-click menus, as the Context menu page arranges it: one list of
/// tokens shared by the file list's and the folder tree's menus, each of which shows the entries it
/// has and skips the rest.
/// </summary>
/// <remarks>
/// <para>
/// The layout decides <em>order only</em>. Whether an entry is shown stays where it always lived —
/// the hidden lists and a command's <c>ShowInMenu</c> — so an arrangement from an older or newer
/// build can never un-hide anything, and a built-in added by a later build appears at its default
/// spot instead of nowhere (<see cref="Normalize"/>).
/// </para>
/// <para>
/// <see cref="More"/> is where everything not placed yet goes: a command made before the menu could
/// be arranged, and, above all, a shell extension installed since the user last visited the page.
/// "Shown unless unticked" is the shell section's rule, and a token list with no catch-all would
/// quietly turn it into "hidden unless placed".
/// </para>
/// </remarks>
public static class MenuLayoutRules
{
    public const string BuiltInPrefix = "app:";
    public const string CommandPrefix = "cmd:";
    public const string SeparatorToken = "-";
    public const string More = "@more";

    public static string BuiltInToken(string id) => BuiltInPrefix + id;
    public static string CommandToken(string id) => CommandPrefix + id;

    private static readonly StringComparer Cmp = StringComparer.OrdinalIgnoreCase;

    /// <summary>The menu as it was before it could be arranged: the file list's order and groups,
    /// with the user's commands and other programs' entries where their anchors were.</summary>
    public static IReadOnlyList<string> Default { get; } =
    [
        "app:new",
        SeparatorToken,
        "app:open", "app:run-as-admin", "app:open-new-tab", "app:open-new-pane", "app:open-terminal",
        "app:open-vscode", "app:disk-usage", "app:duplicates", "app:changes", "app:compare-panes",
        "app:compress", "app:extract",
        SeparatorToken,
        "app:copy-path", "app:copy-name", "app:compare-files", "app:settle-by-content",
        "app:verify-checksums", "app:checksum",
        SeparatorToken,
        "app:cut", "app:copy", "app:paste",
        SeparatorToken,
        "app:rename", "app:delete", "app:delete-permanently", "app:bookmark",
        More,
        SeparatorToken,
        "app:properties",
    ];

    public static bool IsSeparator(string token) => token == SeparatorToken;

    public static bool IsBuiltIn(string token, out string id) => Strip(token, BuiltInPrefix, out id);

    public static bool IsCommand(string token, out string id) => Strip(token, CommandPrefix, out id);

    /// <summary>Anything that is not one of the app's own kinds is a shell extension's id —
    /// <c>clsid:{…}</c> or <c>verb:name</c>, as <see cref="ShellExtension"/> assigns them.</summary>
    public static bool IsShell(string token) =>
        token.Length > 0 && !IsSeparator(token) && !Cmp.Equals(token, More) &&
        !token.StartsWith(BuiltInPrefix, StringComparison.OrdinalIgnoreCase) &&
        !token.StartsWith(CommandPrefix, StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// A stored layout made whole: the default when there is none; every built-in that is neither
    /// placed nor hidden put back after the nearest entry that precedes it by default, and a missing
    /// <see cref="More"/> the same way; any repeat of an entry dropped. A token this
    /// build does not recognise is kept, since it may be a later build's.
    /// </summary>
    public static IReadOnlyList<string> Normalize(
        IReadOnlyList<string>? saved, IEnumerable<string> hiddenBuiltIns)
    {
        if (saved is null) return Default;

        var hidden = new HashSet<string>(hiddenBuiltIns, Cmp);
        var seen = new HashSet<string>(Cmp);
        var result = new List<string>();
        foreach (var raw in saved)
        {
            var token = raw?.Trim() ?? "";
            if (token.Length == 0) continue;
            if (IsSeparator(token) || seen.Add(token)) result.Add(token);
        }

        for (var d = 0; d < Default.Count; d++)
        {
            var token = Default[d];
            if (IsSeparator(token) || seen.Contains(token)) continue;
            if (IsBuiltIn(token, out var id) && hidden.Contains(id)) continue;

            result.Insert(InsertionPoint(result, d), token);
            seen.Add(token);
        }

        return result;
    }

    /// <summary>
    /// One opening of one menu, in order. Each placed entry appears where the layout puts it when
    /// this menu has it; everything unplaced that this menu has appears at <see cref="More"/> —
    /// commands first, then other programs' entries, each run behind a separator, the way the two
    /// sections sat before. Separators are passed through as placed; the view tidies doubled and
    /// dangling ones afterwards, once it knows what is visible.
    /// </summary>
    /// <param name="layout">A layout from <see cref="Normalize"/>.</param>
    /// <param name="builtIns">The built-in ids this menu declares.</param>
    /// <param name="commands">The ids of the commands shown and applicable here, in settings order.</param>
    /// <param name="shellGroups">The extensions with something to offer here, in catalog order.</param>
    public static IReadOnlyList<MenuPlacement> Arrange(
        IReadOnlyList<string> layout,
        IReadOnlySet<string> builtIns,
        IReadOnlyList<string> commands,
        IReadOnlyList<string> shellGroups)
    {
        var commandSet = new HashSet<string>(commands, Cmp);
        var shellSet = new HashSet<string>(shellGroups, Cmp);
        var placedCommands = new HashSet<string>(Cmp);
        var placedShell = new HashSet<string>(Cmp);
        foreach (var token in layout)
        {
            if (IsCommand(token, out var id)) placedCommands.Add(id);
            else if (IsShell(token)) placedShell.Add(token);
        }

        var emitted = new HashSet<string>(Cmp);
        var result = new List<MenuPlacement>();

        foreach (var token in layout)
        {
            if (IsSeparator(token))
            {
                result.Add(MenuPlacement.Separator);
            }
            else if (IsBuiltIn(token, out var builtIn))
            {
                if (builtIns.Contains(builtIn) && emitted.Add(token))
                    result.Add(new MenuPlacement(MenuPlacementKind.BuiltIn, builtIn));
            }
            else if (IsCommand(token, out var command))
            {
                if (commandSet.Contains(command) && emitted.Add(token))
                    result.Add(new MenuPlacement(MenuPlacementKind.Command, command));
            }
            else if (Cmp.Equals(token, More))
            {
                if (!emitted.Add(More)) continue;
                EmitRun(commands.Where(c => !placedCommands.Contains(c)), MenuPlacementKind.Command);
                EmitRun(shellGroups.Where(s => !placedShell.Contains(s)), MenuPlacementKind.Shell);
            }
            else if (shellSet.Contains(token) && emitted.Add(token))
            {
                result.Add(new MenuPlacement(MenuPlacementKind.Shell, token));
            }
        }

        return result;

        void EmitRun(IEnumerable<string> ids, MenuPlacementKind kind)
        {
            var first = true;
            foreach (var id in ids)
            {
                if (first) result.Add(MenuPlacement.Separator);
                first = false;
                result.Add(new MenuPlacement(kind, id));
            }
        }
    }

    /// <summary>Just after the nearest entry that comes before <c>Default[index]</c> by default and
    /// is present; the top when none is.</summary>
    private static int InsertionPoint(List<string> result, int index)
    {
        for (var p = index - 1; p >= 0; p--)
        {
            var before = Default[p];
            if (IsSeparator(before)) continue;
            var at = result.FindIndex(t => Cmp.Equals(t, before));
            if (at >= 0) return at + 1;
        }

        // Nothing before it is present: put it ahead of the first default entry that follows and is.
        for (var n = index + 1; n < Default.Count; n++)
        {
            var after = Default[n];
            if (IsSeparator(after)) continue;
            var at = result.FindIndex(t => Cmp.Equals(t, after));
            if (at >= 0) return at;
        }

        return result.Count;
    }

    private static bool Strip(string token, string prefix, out string id)
    {
        if (token.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            id = token[prefix.Length..];
            return true;
        }

        id = "";
        return false;
    }
}
