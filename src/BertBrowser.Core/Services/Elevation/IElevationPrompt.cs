namespace BertBrowser.Core.Services.Elevation;

/// <summary>What the user is being asked to consent to.</summary>
/// <param name="Items">The paths Windows refused, in the order they were selected. The dialog shows
/// a bounded number of them and says how many more there are — a five-thousand-row list is not a
/// confirmation.</param>
/// <param name="IsUndo">The operation being retried is putting something back rather than doing it,
/// which reads differently and deserves different words.</param>
public sealed record ElevationOffer(
    ElevationOperation Operation,
    IReadOnlyList<string> Items,
    bool IsUndo = false);

/// <summary>
/// Asking whether to try again with an administrator token.
/// </summary>
/// <remarks>
/// <para>
/// A seam rather than a call into a dialog, for the reason <c>IProcessLauncher</c> and
/// <c>IFolderHandlerService</c> are seams: a scripted harness run must be able to answer without a
/// window, and a test must be able to answer without a person.
/// </para>
/// <para>
/// <b>It is consulted from the view model rather than from a view, which is the opposite of where
/// every other failure dialog in this app lives, and that is forced rather than chosen.</b> Each of
/// the four operations claims the one-level undo slot and calls <c>RetireUndoable</c> — the moment
/// staged data is erased — inside itself. A retry raised after the method returned would need a
/// second undo record, and claiming it would retire the first, committing a staging folder the user
/// might still have wanted back. So the offer has to happen inside the same <c>IsTransferring</c>
/// window, before the slot is claimed.
/// </para>
/// </remarks>
public interface IElevationPrompt
{
    /// <summary>True to go ahead and raise the UAC prompt.</summary>
    bool Offer(ElevationOffer offer);
}

/// <summary>Answers no, always. What a context with nobody to ask gets — a background sweep, or a
/// scripted run that must never put a prompt on the user's desktop.</summary>
public sealed class RefusingElevationPrompt : IElevationPrompt
{
    public bool Offer(ElevationOffer offer) => false;
}
