using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Elevation;

namespace BertBrowser.Core.Services.Shortcuts;

/// <summary>
/// Writes one <c>.lnk</c>. The seam that keeps shell COM out of Core, the way
/// <c>IFileCopier</c> keeps the copy loop swappable — the implementation lives in the App beside the
/// other shell interop.
/// </summary>
public interface IShortcutWriter
{
    /// <summary>Creates <paramref name="linkPath"/> pointing at <paramref name="targetPath"/>.
    /// Throws on failure; never overwrites silently is the caller's rule, not this one's.</summary>
    void Write(string linkPath, string targetPath);
}

/// <summary>
/// Carries out a <see cref="ShortcutPlan"/>.
/// </summary>
/// <remarks>
/// <para>
/// The plan's link names are re-checked against live disk before anything is written. The plan was
/// built while a menu sat open under the cursor, and a name that was free then may not be now —
/// writing over it would destroy a file the user never named. This is the same re-check the
/// transfer, rename and new-item executors do, for the same reason.
/// </para>
/// <para>
/// One item's failure never affects the others: each link is written on its own and a refusal is
/// collected rather than thrown.
/// </para>
/// </remarks>
public sealed class ShortcutExecutor
{
    private readonly IShortcutWriter _writer;
    private readonly IShortcutProbe _probe;

    public ShortcutExecutor(IShortcutWriter writer, IShortcutProbe probe)
    {
        _writer = writer;
        _probe = probe;
    }

    public ShortcutExecutor(IShortcutWriter writer) : this(writer, new FileSystemShortcutProbe())
    {
    }

    public ShortcutOutcome Execute(ShortcutPlan plan)
    {
        if (!plan.HasWork) return ShortcutOutcome.Empty;

        var created = new List<string>();
        var failed = new List<FailedShortcut>();

        // Links written by this run count as taken too: the plan reserved names against the disk it
        // saw, and a name it stepped aside from may itself have been freed since.
        var written = new HashSet<string>(StringComparer.Ordinal);

        foreach (var creation in plan.Creations)
        {
            var link = UniquePath.For(
                creation.LinkPath,
                isDirectory: false,
                _probe.DirectoryExists,
                candidate => written.Contains(PathKey.Canonicalize(candidate)) ||
                             _probe.FileExists(candidate));

            try
            {
                _writer.Write(link, creation.TargetPath);
                written.Add(PathKey.Canonicalize(link));
                created.Add(link);
            }
            catch (Exception ex) when (IsWriteFailure(ex))
            {
                failed.Add(new FailedShortcut(
                    creation.LinkPath,
                    $"Could not create a shortcut to '{Path.GetFileName(creation.TargetPath)}' — {ex.Message}",
                    AccessDenied.Caused(ex)));
            }
        }

        return new ShortcutOutcome(created, failed);
    }

    /// <summary>Errors that mean "this failed" rather than "the program is broken". The COM writer
    /// surfaces a refusal as a <see cref="System.Runtime.InteropServices.COMException"/>, which an
    /// <see cref="IOException"/>-only guard would let take the process down.</summary>
    private static bool IsWriteFailure(Exception ex) =>
        ex is IOException or UnauthorizedAccessException or ArgumentException
            or NotSupportedException or System.Security.SecurityException
            or System.Runtime.InteropServices.COMException;
}
