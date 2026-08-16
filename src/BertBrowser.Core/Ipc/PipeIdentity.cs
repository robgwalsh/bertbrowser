namespace BertBrowser.Core.Ipc;

/// <summary>
/// Comparing the two forms Windows hands back for "who is on the other end of this pipe".
/// </summary>
/// <remarks>
/// <b>The two names are not in the same form.</b> <c>NamedPipeServerStream.GetImpersonationUserName</c>
/// returns the bare account ("Rob"), while <c>WindowsIdentity.Name</c> is qualified
/// ("DESKTOP-K0BI3BS\Rob"). Comparing them whole never matches — and it fails <em>closed</em>, so
/// the symptom is not an error but a pipe that accepts every connection and immediately drops it.
/// That cost a real afternoon once: hand-offs looked like they worked while a second full copy of
/// the app started anyway. One home for the rule, and one comment, so it cannot be rediscovered.
/// <para>
/// This is defence in depth and not a boundary. The DACL is the real gate; nothing between two
/// processes of the same user ever is one.
/// </para>
/// </remarks>
public static class PipeIdentity
{
    /// <summary>True when two Windows account names name the same account, in either form.</summary>
    public static bool SameAccount(string? left, string? right)
    {
        if (left is null || right is null) return false;
        return AccountPart(left).Equals(AccountPart(right), StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>The account portion of a name that may or may not carry a domain prefix.</summary>
    public static string AccountPart(string name) => name[(name.LastIndexOf('\\') + 1)..];
}
