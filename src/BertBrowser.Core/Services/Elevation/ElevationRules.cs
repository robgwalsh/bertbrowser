using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Delete;

namespace BertBrowser.Core.Services.Elevation;

/// <summary>
/// What may never be done with an administrator token, however the user answers the prompt.
/// </summary>
/// <remarks>
/// <para>
/// <b>The refusal belongs where the privilege is.</b> Two planners were relying, explicitly, on the
/// app being <c>asInvoker</c>: <c>TransferPlanner</c> refuses drive roots and nothing else, so
/// dragging <c>C:\Windows</c> onto another folder is refused today only by its ACL; and
/// <c>NewItemPlanner</c> says in so many words that it does not consult
/// <see cref="ProtectedLocations"/> because "<c>C:\Windows</c> is refused by its ACL now the app is
/// <c>asInvoker</c>". A shield button would quietly undo the first of those.
/// </para>
/// <para>
/// The answer is not to tighten the planners. Creating a file in the profile root is the ordinary
/// thing this app is for, and refusing it would cost legitimate unelevated work to guard against a
/// case that only exists one layer up. So the check lives at the escalation boundary instead: the
/// unelevated path is exactly what it always was, and the extra rule applies only where the extra
/// privilege does.
/// </para>
/// <para>
/// It is asked about the item being <em>acted on</em>, never the destination. Copying into
/// <c>C:\Program Files</c> is the headline case for this whole feature; moving <c>C:\Program Files</c>
/// is not.
/// </para>
/// </remarks>
public static class ElevationRules
{
    /// <summary>
    /// True for something that must not be moved, renamed or deleted with a token — the folders
    /// <see cref="ProtectedLocations.Default"/> names, and anything inside a Recycle Bin.
    /// </summary>
    /// <remarks>
    /// The Recycle Bin clause is not symmetry for its own sake: those <c>$R</c> files are what a
    /// pending Ctrl+Z restores from, and an elevated operation is exactly the one with enough rights
    /// to reach another account's.
    /// </remarks>
    public static bool IsRefusedForElevation(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return true;
        if (ProtectedLocations.IsInsideRecycleBin(path)) return true;

        var key = KeyOf(path);
        if (key is null) return true;

        foreach (var protectedKey in ProtectedLocations.Default)
            if (string.Equals(key, protectedKey, StringComparison.Ordinal)) return true;

        return false;
    }

    /// <summary>The database key for a path, or null when it is not a path at all — which is itself
    /// a refusal, since nothing unparseable should reach a process holding a token.</summary>
    internal static string? KeyOf(string path)
    {
        try
        {
            return PathKey.Canonicalize(path);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return null;
        }
    }
}
