using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.FlatView;

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
    public static FlatViewPreflight Decide(string displayPath, DirSizeResult? estimate, int cap)
    {
        if (estimate is null) return FlatViewPreflight.Run;

        // Files only, because that is all a flat view lists — its folders are walked, never shown.
        var count = estimate.FileCount;
        if (count <= cap) return FlatViewPreflight.Run;

        // "at least", because an incomplete row is a floor: the size pass could not reach part of
        // the tree, so the real number is this one or larger, never smaller.
        var floor = estimate.Incomplete ? "at least " : "";

        return new FlatViewPreflight(
            Confirm: true,
            Message:
                $"\"{displayPath}\" holds {floor}{count:N0} files. " +
                $"A flat view lists the first {cap:N0} of them, which will take a moment.",
            ConfirmLabel: $"Show {cap:N0}");
    }
}
