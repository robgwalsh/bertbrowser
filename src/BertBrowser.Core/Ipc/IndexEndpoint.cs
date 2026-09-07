using System.Security.Principal;

namespace BertBrowser.Core.Ipc;

/// <summary>
/// The name of the index pipe, derived the same way in both processes.
/// </summary>
/// <remarks>
/// <para>
/// <b>The helper is no longer told which pipe to call back on — it works it out.</b> That used to
/// be an argument carrying a random nonce, which was right while the helper died with the app that
/// launched it: a fresh attempt could not collide with an endpoint a previous one had left behind.
/// It stopped being right once the helper started outliving apps, because a helper from an earlier
/// app has to be able to find the <em>next</em> one, and it cannot guess a nonce.
/// </para>
/// <para>
/// Deriving it from the process's own token is a better answer than validating an argument:
/// <b>the elevated process can no longer be pointed at a name anybody chose.</b> UAC hands the same
/// user a different token, not a different account, so the SID the helper computes is the SID the
/// app computed, and the two meet without either trusting the other's input.
/// </para>
/// <para>
/// There is deliberately no fallback for a token with no SID. The old one substituted a constant,
/// which was harmless while the name also carried a nonce and is not harmless now: two users whose
/// tokens both failed to report a SID would derive the same name, and the DACL is the only thing
/// that would then keep them apart. Failing loudly is the smaller problem.
/// </para>
/// </remarks>
public static class IndexEndpoint
{
    /// <summary>The prefix every name this app generates begins with.</summary>
    public const string Prefix = "BertBrowser.Index.";

    /// <summary>The pipe name for the current user's helper.</summary>
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    public static string ForCurrentUser() => ForUser(CurrentUserSid());

    /// <summary>The pipe name for a given user's SID.</summary>
    public static string ForUser(string userSid)
    {
        if (!IndexerPresenceLock.IsAcceptableSid(userSid))
            throw new ArgumentException("Not a SID this app would have produced.", nameof(userSid));

        return Prefix + userSid;
    }

    /// <summary>This process's user SID, which is the same under an elevated token as a filtered one.</summary>
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    public static string CurrentUserSid()
    {
        using var identity = WindowsIdentity.GetCurrent();
        return identity.User?.Value
            ?? throw new InvalidOperationException("No user SID for the current process.");
    }
}
