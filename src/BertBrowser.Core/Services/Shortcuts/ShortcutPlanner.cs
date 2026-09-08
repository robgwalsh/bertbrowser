using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Shortcuts;

/// <summary>The filesystem questions <see cref="ShortcutPlanner"/> asks, abstracted so the naming
/// and collision rules can be unit-tested without writing <c>.lnk</c> files.</summary>
public interface IShortcutProbe
{
    bool DirectoryExists(string path);

    bool FileExists(string path);
}

/// <summary>Real-filesystem <see cref="IShortcutProbe"/>.</summary>
public sealed class FileSystemShortcutProbe : IShortcutProbe
{
    public bool DirectoryExists(string path) => Directory.Exists(path);

    public bool FileExists(string path) => File.Exists(path);
}

/// <summary>
/// Works out which <c>.lnk</c> files "Create shortcuts here" would write, and where.
/// </summary>
/// <remarks>
/// <para>
/// The naming is Explorer's, extension included: <c>notes.txt</c> becomes
/// <c>notes.txt - Shortcut.lnk</c>, and a name already taken steps aside through
/// <see cref="UniquePath"/> to <c>notes.txt - Shortcut (2).lnk</c> — the same "(2)" the rest of the
/// app produces for a paste or a New File.
/// </para>
/// <para>
/// <b>Names are reserved as the plan is built</b>, not merely probed. One drag can carry two files
/// called <c>notes.txt</c> from different folders; nothing is on disk yet, so a planner that only
/// asked the probe would hand both the same link path and the second write would replace the first.
/// </para>
/// <para>
/// A source that is not on disk is refused, and that single rule is what keeps items inside an
/// archive out: <c>C:\x\bundle.zip\notes.txt</c> reads like a path but names nothing a shortcut
/// could resolve. No archive-specific check is needed, and none is here — the honest question is
/// "is there something to point at", and there is not.
/// </para>
/// </remarks>
public sealed class ShortcutPlanner
{
    private const string Suffix = " - Shortcut";
    private const string Extension = ".lnk";

    private readonly IShortcutProbe _probe;

    public ShortcutPlanner(IShortcutProbe probe) => _probe = probe;

    public ShortcutPlanner() : this(new FileSystemShortcutProbe())
    {
    }

    /// <summary>What creating shortcuts to <paramref name="sources"/> in
    /// <paramref name="directory"/> would produce. Never throws: an unusable path is a refusal like
    /// any other.</summary>
    public ShortcutPlan Plan(IReadOnlyList<string> sources, string directory)
    {
        if (!IsUsable(directory) || !_probe.DirectoryExists(directory))
        {
            return new ShortcutPlan(
                directory,
                [],
                sources.Count == 0
                    ? []
                    : [new RejectedShortcut("", "The folder to create the shortcuts in is no longer there.")]);
        }

        var creations = new List<ShortcutCreation>();
        var problems = new List<RejectedShortcut>();

        // Link paths this plan has already claimed. Canonicalized, because two sources may differ
        // only in case and "notes.txt" must still collide with "NOTES.TXT".
        var reserved = new HashSet<string>(StringComparer.Ordinal);

        foreach (var source in sources)
        {
            if (!IsUsable(source) || !(_probe.FileExists(source) || _probe.DirectoryExists(source)))
            {
                problems.Add(new RejectedShortcut(
                    source, $"'{Path.GetFileName(source)}' is no longer there."));
                continue;
            }

            var name = Path.GetFileName(source.TrimEnd(Path.DirectorySeparatorChar));
            if (name.Length == 0)
            {
                // A drive root has no file name to build one from.
                problems.Add(new RejectedShortcut(
                    source, "A drive root has no name to make a shortcut from."));
                continue;
            }

            var link = UniquePath.For(
                Path.Combine(directory, name + Suffix + Extension),
                isDirectory: false,
                _probe.DirectoryExists,
                candidate => reserved.Contains(PathKey.Canonicalize(candidate)) ||
                             _probe.FileExists(candidate));

            reserved.Add(PathKey.Canonicalize(link));
            creations.Add(new ShortcutCreation(source, link));
        }

        return new ShortcutPlan(directory, creations, problems);
    }

    /// <summary>Whether a path can be canonicalized at all. The other planners swallow exactly these
    /// three for the same reason: an unusable path is a refusal to show, not an exception.</summary>
    private static bool IsUsable(string path)
    {
        try
        {
            PathKey.Canonicalize(path);
            return true;
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException
                                       or PathTooLongException)
        {
            return false;
        }
    }
}
