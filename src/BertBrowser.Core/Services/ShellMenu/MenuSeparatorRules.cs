namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>One slot of a menu as the separator rule sees it: a separator, or anything else.</summary>
/// <param name="IsVisible">For an item, whether it is showing. Ignored for a separator: the rule
/// decides those from scratch each time.</param>
public readonly record struct MenuSlot(bool IsSeparator, bool IsVisible);

/// <summary>
/// Which separators of a menu should show once the user has unticked some of its entries. The
/// separators are declared in XAML between groups; when a whole group is hidden two of them meet,
/// and when the first or last group goes one is left at the edge.
/// </summary>
public static class MenuSeparatorRules
{
    /// <summary>Per slot: whether it should be visible. Items keep their own answer; a separator
    /// shows only between two visible items, and only the first of a run.</summary>
    public static IReadOnlyList<bool> Apply(IReadOnlyList<MenuSlot> slots)
    {
        var visible = new bool[slots.Count];
        var pendingSeparator = -1;
        var anyItemBefore = false;

        for (var i = 0; i < slots.Count; i++)
        {
            var slot = slots[i];
            if (slot.IsSeparator)
            {
                // Remembered, not shown: it shows only once an item follows it.
                if (anyItemBefore && pendingSeparator < 0) pendingSeparator = i;
                continue;
            }

            if (!slot.IsVisible) continue;

            visible[i] = true;
            if (pendingSeparator >= 0)
            {
                visible[pendingSeparator] = true;
                pendingSeparator = -1;
            }
            anyItemBefore = true;
        }

        return visible;
    }
}
