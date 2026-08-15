namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// The shell's <c>DROPEFFECT</c> values, mirrored here so the drag-out rule can live in Core.
/// <see cref="Scroll"/> is a modifier bit the shell sets during auto-scroll and is never a verb.
/// </summary>
[Flags]
public enum DropEffect
{
    None = 0,
    Copy = 1,
    Move = 2,
    Link = 4,
    Scroll = unchecked((int)0x80000000),
}

/// <summary>What the drag source has to do once a drag has ended.</summary>
public enum DragOutAction
{
    /// <summary>Nothing happened, or we already did the work ourselves.</summary>
    Nothing,

    /// <summary>Nothing to remove, but the folder may have changed underneath us — reload it.</summary>
    RefreshOnly,

    /// <summary>A foreign target copied the items and expects the source to remove the originals.</summary>
    RemoveSources,
}

/// <summary>
/// Whether a finished drag means "remove the originals", which is the one genuinely dangerous
/// question in dragging files out of this app.
/// </summary>
/// <remarks>
/// <para>
/// <c>DoDragDrop</c> returning <see cref="DropEffect.Move"/> does <b>not</b> mean the source should
/// delete anything. The shell's protocol has three cases:
/// </para>
/// <list type="number">
/// <item>The target performed an <b>optimized move</b> — it relocated the files itself, which is
/// what Explorer does within a volume — and reports that by writing
/// <c>CFSTR_PERFORMEDDROPEFFECT</c> = <see cref="DropEffect.None"/> back onto our data object. The
/// source must do nothing; the files are already gone.</item>
/// <item><c>CFSTR_LOGICALPERFORMEDDROPEFFECT</c> exists because some targets report <c>MOVE</c> in
/// the older format even for an optimized move, so it wins wherever both are present.</item>
/// <item>Only a <b>non-optimized move</b> — the target copied and left the originals — puts the
/// removal on the source.</item>
/// </list>
/// <para>
/// Getting this wrong is expensive in both directions: doing nothing silently turns every
/// non-optimized move into a copy, and removing too eagerly destroys the user's files on the say-so
/// of an arbitrary external window. So the whole decision is this one pure function with a truth
/// table behind it in <c>DragOutContractTests</c>, and the removal it asks for goes through the
/// ordinary reversible delete rather than anything that erases.
/// </para>
/// <para>
/// The deliberate asymmetry: we remove sources <b>only</b> when told <c>Move</c> or when a target
/// reports nothing at all (the documented fallback, and what makes a real move work with targets
/// that ignore the protocol). Any other answer — including a target that contradicts itself — falls
/// back to leaving the files alone.
/// </para>
/// </remarks>
public static class DragOutContract
{
    /// <summary>Verb bits only: the shell sets <see cref="DropEffect.Scroll"/> during auto-scroll,
    /// and a raw comparison against <see cref="DropEffect.Move"/> would miss a value carrying it.</summary>
    private const DropEffect Verbs = DropEffect.Copy | DropEffect.Move | DropEffect.Link;

    /// <param name="handledInApp">True when this app's own drop pipeline took the drop, in which
    /// case the transfer has already happened and the originals are already where they belong.</param>
    /// <param name="returned">What <c>DoDragDrop</c> returned.</param>
    /// <param name="logicalPerformed">
    /// <c>CFSTR_LOGICALPERFORMEDDROPEFFECT</c> read back off the data object, or null if absent.</param>
    /// <param name="performed"><c>CFSTR_PERFORMEDDROPEFFECT</c>, or null if absent.</param>
    public static DragOutAction Decide(
        bool handledInApp,
        DropEffect returned,
        DropEffect? logicalPerformed,
        DropEffect? performed)
    {
        // Our own pipeline already moved or copied the items through TransferExecutor. Acting on
        // the returned effect here would delete what we just placed.
        if (handledInApp) return DragOutAction.Nothing;

        var verb = returned & Verbs;
        if (verb == DropEffect.None) return DragOutAction.Nothing;   // cancelled, or refused
        if ((verb & DropEffect.Move) == 0) return DragOutAction.RefreshOnly;  // copy or link

        // Logical wins over the legacy format wherever both are present; if neither target format
        // came back, the return value is all we have and the documented reading of it is "move".
        var reported = logicalPerformed ?? performed;
        if (reported is not { } signal) return DragOutAction.RemoveSources;

        // None means an optimized move: the target already relocated them. Anything else that is
        // not Move (a target contradicting itself) is not a licence to delete.
        return (signal & DropEffect.Move) != 0
            ? DragOutAction.RemoveSources
            : DragOutAction.RefreshOnly;
    }
}
