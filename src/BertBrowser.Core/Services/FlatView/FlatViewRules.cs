using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.FlatView;

/// <summary>
/// What a tab's flat branch view is showing.
/// </summary>
/// <remarks>
/// A mode rather than a bool because the two shapes answer different questions, and both are worth
/// having: <see cref="Files"/> is the one that makes a tree of nested media browsable, since a
/// folder row in a flat list is one more thing between you and what you were looking for;
/// <see cref="All"/> is "the subtree, listed", and keeps the things folder rows carry — a drop
/// target, and a cached recursive size.
/// </remarks>
public enum FlatViewMode
{
    /// <summary>An ordinary one-level directory listing.</summary>
    Off,

    /// <summary>Every file under the folder, at every depth. No folder rows.</summary>
    Files,

    /// <summary>Every file and folder under the folder, at every depth.</summary>
    All,
}

/// <summary>Whether to list a subtree flat straight away, or ask first.</summary>
/// <param name="Confirm">True when the user should be asked before anything is listed.</param>
/// <param name="Message">What to ask, in words. Empty when there is nothing to ask.</param>
/// <param name="ConfirmLabel">What the button that goes ahead should say.</param>
public sealed record FlatViewPreflight(bool Confirm, string Message, string ConfirmLabel)
{
    public static readonly FlatViewPreflight Run = new(false, "", "");
}

/// <summary>
/// The decisions a flat branch view makes before it walks anything.
/// </summary>
/// <remarks>
/// Pure, and separate from the view model for the reason every planner here is: the threshold and
/// the wording are the parts worth holding still, and a test can hold them still without a window.
/// </remarks>
public static class FlatViewRules
{
    /// <summary>
    /// Whether to go ahead, given what the index already knows about the folder's size.
    /// </summary>
    /// <param name="estimate">
    /// The <c>dir_size_cache</c> row for the folder, or null.
    /// <para>
    /// <b>Null means go ahead.</b> A missing row means <em>unknown</em> — the standing rule for this
    /// table, the same one that makes a folder's size render blank rather than zero — so there is
    /// nothing to warn about and the cap plus its banner carry it instead. That is also what happens
    /// on every drive the MFT pass has not measured: a network share, a non-NTFS volume, a tree
    /// indexed by the names-only USN fallback.
    /// </para>
    /// <para>
    /// It is only ever an <em>estimate</em>, and that is the whole reason the index may answer it
    /// while it may not answer the listing itself. A count that is a rebuild out of date changes
    /// only whether a question gets asked; a row that is out of date would be a lie about what is on
    /// the disk. The rows come from a live walk for exactly that reason.
    /// </para>
    /// </param>
    public static FlatViewPreflight Decide(
        string displayPath, DirSizeResult? estimate, FlatViewMode mode, int cap)
    {
        if (mode == FlatViewMode.Off || estimate is null) return FlatViewPreflight.Run;

        var count = mode == FlatViewMode.All
            ? (long)estimate.FileCount + estimate.DirCount
            : estimate.FileCount;

        if (count <= cap) return FlatViewPreflight.Run;

        // "at least", because an incomplete row is a floor: the size pass could not reach part of
        // the tree, so the real number is this one or larger, never smaller.
        var floor = estimate.Incomplete ? "at least " : "";
        var what = mode == FlatViewMode.All ? "files and folders" : "files";

        return new FlatViewPreflight(
            Confirm: true,
            Message:
                $"\"{displayPath}\" holds {floor}{count:N0} {what}. " +
                $"A flat view lists the first {cap:N0} of them, which will take a moment.",
            ConfirmLabel: $"Show {cap:N0}");
    }
}
