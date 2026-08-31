using System.Security.AccessControl;
using System.Runtime.Versioning;

namespace BertBrowser.Harness;

/// <summary>
/// A file Windows will genuinely refuse to delete or move, made without any privilege at all.
/// </summary>
/// <remarks>
/// <para>
/// This is what lets a scripted run exercise the elevation offer for real: the discriminator, the
/// rules, the merge and the dialog are all reached by an ordinary <c>delete</c> against a file
/// carrying a Deny ACE, with no token and no prompt anywhere in it.
/// </para>
/// <para>
/// <b>The denial goes on the folder, before the file is created, and both halves matter.</b> Windows
/// lets a file be deleted when its parent grants <c>FILE_DELETE_CHILD</c>, whatever the file's own
/// DACL says — so denying only the file is simply ignored. And adding an inheritable ACE to a folder
/// does <em>not</em> rewrite the DACL of a file already sitting in it: that file keeps the full
/// control it inherited when it was made, that grant alone is enough, and the denial is never
/// consulted. Creating the file underneath an already-denied folder is the only version of this that
/// reproduces what a protected file looks like.
/// </para>
/// <para>
/// Every denial is lifted on the way out, or the run's scratch directory cannot be removed and every
/// later run leaves another one behind.
/// </para>
/// </remarks>
[SupportedOSPlatform("windows")]
internal sealed class DeniedFixture
{
    private readonly List<string> _folders = [];

    /// <summary>Creates <paramref name="path"/> as a file the current account may not delete or
    /// move. The folder holding it is denied, so anything else created in it afterwards is denied
    /// too — use a folder of its own.</summary>
    internal void Deny(string path)
    {
        var folder = Path.GetDirectoryName(path)
            ?? throw new ArgumentException($"'{path}' has no parent folder.");
        Directory.CreateDirectory(folder);

        var info = new DirectoryInfo(folder);
        var security = info.GetAccessControl();
        security.AddAccessRule(Denial());
        info.SetAccessControl(security);
        _folders.Add(folder);

        if (!File.Exists(path)) File.WriteAllText(path, "denied");
    }

    /// <summary>Lifts every denial this fixture applied. Never throws: it runs during teardown,
    /// where a failure must not mask whatever the run was actually about.</summary>
    internal void Release()
    {
        foreach (var folder in _folders)
        {
            try
            {
                var info = new DirectoryInfo(folder);
                var security = info.GetAccessControl();
                security.RemoveAccessRuleAll(Denial());
                info.SetAccessControl(security);

                // The children kept the ACE they inherited when they were made; letting them
                // re-inherit from the folder above is what makes the tree removable again.
                foreach (var path in Directory.EnumerateFiles(folder))
                {
                    var file = new FileInfo(path);
                    var fileSecurity = file.GetAccessControl();
                    fileSecurity.SetAccessRuleProtection(isProtected: false, preserveInheritance: false);
                    file.SetAccessControl(fileSecurity);
                }
            }
            catch (Exception ex) when (
                ex is IOException or UnauthorizedAccessException or ArgumentException
                    or PrivilegeNotHeldException)
            {
            }
        }

        _folders.Clear();
    }

    /// <summary>Delete rights only. Write is left alone so the fixture can still create the file
    /// after the denial is in place.</summary>
    private static FileSystemAccessRule Denial() =>
        new(Environment.UserName,
            FileSystemRights.Delete | FileSystemRights.DeleteSubdirectoriesAndFiles,
            InheritanceFlags.ObjectInherit | InheritanceFlags.ContainerInherit,
            PropagationFlags.None,
            AccessControlType.Deny);
}
