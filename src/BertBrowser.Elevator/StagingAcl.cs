using System.Security.AccessControl;
using System.Security.Principal;

namespace BertBrowser.Elevator;

/// <summary>
/// Handing back what this process created, so the unelevated app can still finish with it.
/// </summary>
/// <remarks>
/// <para>
/// <b>This is the one piece of the elevated path with no test behind it and a silent failure mode,
/// so it is worth understanding rather than skimming.</b> A staged delete puts the user's data in
/// <c>&lt;volume root&gt;\.bertbrowser-trash\delete-&lt;id&gt;</c>, and a Replace puts the displaced
/// entry in <c>.bertbrowser-replaced-*</c>. Both are created by whichever process is doing the work.
/// A folder created at a volume root inherits that root's ACL, which grants ordinary users read and
/// create but <em>not</em> delete — so a holding folder this process makes is one the app cannot
/// later commit (<c>CommitStaging</c>), cannot purge (<c>PurgeAbandonedStaging</c>), and cannot move
/// an item back out of when the user presses Ctrl+Z.
/// </para>
/// <para>
/// The database's note about Administrators-owned files does not cover this and should not be read
/// as covering it: that reasoning is about the profile directory, which carries inheritable full
/// control for the interactive user. A volume root does not.
/// </para>
/// <para>
/// So every staging folder the run created gets an inheritable full-control grant for the account
/// that asked for the operation. Verify it by hand with <c>icacls</c> after an elevated staged
/// delete — the interactive user must have <c>(F)</c>, and inherited.
/// </para>
/// </remarks>
[System.Runtime.Versioning.SupportedOSPlatform("windows")]
internal static class StagingAcl
{
    /// <summary>A grant for the given SID, ready to hand to <c>ElevationHost</c>. Never throws: a
    /// failure here costs the app the ability to tidy a hidden folder, which must not be allowed to
    /// take down an operation that has already moved the user's data.</summary>
    internal static Action<string> GrantTo(string userSid) => directory =>
    {
        try
        {
            var identity = new SecurityIdentifier(userSid);
            var info = new DirectoryInfo(directory);
            if (!info.Exists) return;

            var security = info.GetAccessControl();
            security.AddAccessRule(new FileSystemAccessRule(
                identity,
                FileSystemRights.FullControl,
                InheritanceFlags.ObjectInherit | InheritanceFlags.ContainerInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
            info.SetAccessControl(security);
        }
        catch (Exception ex) when (
            ex is IOException or UnauthorizedAccessException or ArgumentException
                or PrivilegeNotHeldException or PlatformNotSupportedException)
        {
            Console.Error.WriteLine($"Could not hand back '{directory}': {ex.Message}");
        }
    };
}
