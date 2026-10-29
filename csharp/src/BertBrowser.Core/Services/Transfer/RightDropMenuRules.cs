namespace BertBrowser.Core.Services.Transfer;

/// <summary>What the menu at the end of a right-drag can offer.</summary>
public enum RightDropVerb
{
    Move,

    Copy,

    /// <summary>Leave the items where they are and put a <c>.lnk</c> pointing at each one here.</summary>
    Shortcut,
}

/// <param name="Verb">What picking this entry would do.</param>
/// <param name="Label">The wording, count-aware.</param>
/// <param name="Enabled">False when the verb has nothing to do here, or cannot apply at all.</param>
public readonly record struct RightDropEntry(RightDropVerb Verb, string Label, bool Enabled);

/// <param name="DestinationIsArchive">The drop lands inside a container, which is rewritten to add
/// the files rather than transferred into.</param>
/// <param name="SourcesAreVirtual">The items being dragged live inside a container, so their paths
/// name nothing on disk.</param>
/// <param name="MoveHasWork">A planned move would do something.</param>
/// <param name="CopyHasWork">A planned copy would do something.</param>
/// <param name="ItemCount">How many items are being dragged, for the wording.</param>
public readonly record struct RightDropContext(
    bool DestinationIsArchive,
    bool SourcesAreVirtual,
    bool MoveHasWork,
    bool CopyHasWork,
    int ItemCount);

/// <summary>
/// Explorer's right-drag menu: the verbs offered when the right button is released over a drop
/// target, rather than a verb chosen for the user by the modifier keys they happened to be holding.
/// </summary>
/// <remarks>
/// <para>
/// Entries are <em>greyed, never removed</em>. The menu opens under the cursor at the end of a
/// gesture, so an item that is present on one drop and absent on the next moves everything below it
/// — and the thing below Copy is a verb that leaves the originals behind. A stable menu costs one
/// disabled row; an unstable one costs a misclick.
/// </para>
/// <para>
/// Nothing here decides whether a transfer is <em>safe</em>: that stays with
/// <c>TransferPlanner</c>, which is re-run from scratch once a verb is picked. These flags come from
/// a hover-time plan and are advisory, exactly as the drag cursor is.
/// </para>
/// </remarks>
public static class RightDropMenuRules
{
    public static IReadOnlyList<RightDropEntry> Build(RightDropContext context)
    {
        // A container is rewritten to add files to it. There is no move — the entry would have to
        // delete out of the source folder on the strength of a rewrite — and no shortcut, because a
        // .lnk stores a path and an entry inside a container has not got one.
        var archive = context.DestinationIsArchive;

        return
        [
            new RightDropEntry(RightDropVerb.Move, "Move here", !archive && context.MoveHasWork),
            new RightDropEntry(RightDropVerb.Copy, "Copy here", context.CopyHasWork),
            new RightDropEntry(
                RightDropVerb.Shortcut,
                context.ItemCount == 1 ? "Create shortcut here" : "Create shortcuts here",
                !archive && !context.SourcesAreVirtual && context.ItemCount > 0),
        ];
    }
}
