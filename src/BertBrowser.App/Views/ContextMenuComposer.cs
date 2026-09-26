using System.Runtime.CompilerServices;
using System.Windows.Controls;
using BertBrowser.App.Services;
using BertBrowser.Core.Services.ShellMenu;

namespace BertBrowser.App.Views;

/// <summary>
/// Puts a right-click menu in the order the Context menu page arranged: the app's own items
/// (declared in XAML with <c>Tag="id"</c>), separators, the user's commands and other programs'
/// entries, as <see cref="MenuLayoutRules.Arrange"/> lays them out for this opening. Shared by the
/// folder tree's menu and every pane's file-list menu, which is what makes one arrangement serve
/// both — each has the items it declares and skips the rest.
/// </summary>
/// <remarks>
/// The declared items are indexed once, the first time a menu is seen, and re-added every opening
/// rather than recreated: they are the same objects the code-behind names and enables, so an item
/// the layout leaves out is merely not in the menu this time. Separators are never declared; every
/// one is the layout's.
/// </remarks>
internal static class ContextMenuComposer
{
    private static readonly ConditionalWeakTable<ContextMenu, Dictionary<string, List<MenuItem>>> Declared = new();

    /// <summary>Every item the menu declares, whether or not it is in the menu right now — what
    /// <see cref="BuiltInMenu.Apply"/> has to reach, since an item left out of the last opening is
    /// not in <c>Items</c> but may be in the next.</summary>
    public static IEnumerable<MenuItem> BuiltInItems(ContextMenu menu) => Index(menu).Values.SelectMany(v => v);

    /// <summary>Rebuilds the menu's items for one opening. Last but one: every declared item's
    /// visibility is already decided, and <see cref="BuiltInMenu.TidySeparators"/> follows.</summary>
    public static void Compose(
        ContextMenu menu,
        AppSettings settings,
        IReadOnlyList<(string FullPath, bool IsDirectory)> commandTargets,
        Action<CustomCommandDefinition, IReadOnlyList<(string FullPath, bool IsDirectory)>> runCommand,
        ShellMenuSession? session,
        Func<IntPtr> ownerWindow,
        Action<string> report)
    {
        var declared = Index(menu);

        var commands = settings.CustomCommands
            .Where(c => c.ShowInMenu &&
                        commandTargets.Any(t => t.IsDirectory ? c.AppliesToDirectories : c.AppliesToFiles))
            .GroupBy(c => c.Id, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(g => g.Key, g => g.First(), StringComparer.OrdinalIgnoreCase);
        var groups = (session?.Groups ?? [])
            .GroupBy(g => g.ExtensionId, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(g => g.Key, g => g.First(), StringComparer.OrdinalIgnoreCase);

        var placements = MenuLayoutRules.Arrange(
            MenuLayoutRules.Normalize(settings.ContextMenuLayout, settings.HiddenBuiltInMenuItems),
            declared.Keys.ToHashSet(StringComparer.OrdinalIgnoreCase),
            settings.CustomCommands.Where(c => commands.ContainsKey(c.Id)).Select(c => c.Id).Distinct().ToList(),
            (session?.Groups ?? []).Select(g => g.ExtensionId).ToList());

        menu.Items.Clear();
        foreach (var placement in placements)
        {
            switch (placement.Kind)
            {
                case MenuPlacementKind.Separator:
                    menu.Items.Add(new Separator());
                    break;
                case MenuPlacementKind.BuiltIn:
                    foreach (var item in declared[placement.Id]) menu.Items.Add(item);
                    break;
                case MenuPlacementKind.Command:
                    menu.Items.Add(CustomCommandMenu.Build(commands[placement.Id], commandTargets, runCommand));
                    break;
                case MenuPlacementKind.Shell:
                    foreach (var entry in groups[placement.Id].Entries)
                        menu.Items.Add(ShellMenu.Build(entry, session!, ownerWindow, report));
                    break;
            }
        }
    }

    private static Dictionary<string, List<MenuItem>> Index(ContextMenu menu) =>
        Declared.GetValue(menu, m =>
        {
            var index = new Dictionary<string, List<MenuItem>>(StringComparer.OrdinalIgnoreCase);
            foreach (var item in m.Items.OfType<MenuItem>())
            {
                if (item.Tag is not string id) continue;
                if (!BuiltInMenuItems.IsKnown(id))
                    throw new InvalidOperationException($"'{id}' is not one of the app's menu entries.");
                if (!index.TryGetValue(id, out var items)) index[id] = items = [];
                items.Add(item);
            }
            return index;
        });
}
