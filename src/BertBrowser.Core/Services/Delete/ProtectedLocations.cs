using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// The handful of folders this app will not delete however they were selected.
/// </summary>
/// <remarks>
/// <para>
/// <b>The profile root is why this list still exists.</b> The app runs as the user now, so Windows
/// itself refuses a delete of <c>Windows</c> or <c>Program Files</c> — but
/// <see cref="Environment.SpecialFolder.UserProfile"/> is entirely writable by its owner, and a
/// stray Ctrl+A there would find nothing in the way. That one entry earns the list on its own.
/// </para>
/// <para>
/// The system folders stay as belt and braces: cheap, and they keep the rule honest on a machine
/// where the app has somehow been started elevated anyway.
/// </para>
/// </remarks>
/// <remarks>
/// Only the folders themselves are protected, not what is inside them: the point is to refuse the
/// one selection that ends the machine, not to make the app decline to manage files. Drive and
/// volume roots are refused separately by the planner, which is what makes exact matching enough
/// here — every entry below sits directly under a root that is already unreachable.
/// </remarks>
public static class ProtectedLocations
{
    /// <summary>The per-volume Recycle Bin folder, and the name Windows used before it.</summary>
    private static readonly string[] RecycleBinFolders = ["$Recycle.Bin", "$RECYCLE.BIN", "RECYCLER"];

    /// <summary>Canonical keys of the locations refused by default.</summary>
    public static IReadOnlyCollection<string> Default { get; } = Build();

    /// <summary>
    /// True for a Recycle Bin folder or anything inside one. Unlike the exact matches above this
    /// covers the contents too, and deliberately: those <c>$R</c> files are what Ctrl+Z restores
    /// from, so deleting one out from under a pending undo would break it. It is a name test rather
    /// than a path list because the folder exists once per volume and enumerating drives to find
    /// them would mean touching every disconnected network share at startup.
    /// </summary>
    public static bool IsInsideRecycleBin(string path)
    {
        foreach (var segment in path.Split(['\\', '/'], StringSplitOptions.RemoveEmptyEntries))
        {
            foreach (var folder in RecycleBinFolders)
                if (string.Equals(segment, folder, StringComparison.OrdinalIgnoreCase)) return true;
        }
        return false;
    }

    /// <summary>Canonicalizes <paramref name="paths"/> into a set the planner can test against,
    /// dropping anything that isn't a usable path.</summary>
    public static HashSet<string> KeysOf(IEnumerable<string> paths)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var path in paths)
        {
            if (string.IsNullOrWhiteSpace(path)) continue;
            try
            {
                keys.Add(PathKey.Canonicalize(path));
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                // Not a path we could ever be asked to delete either.
            }
        }
        return keys;
    }

    private static HashSet<string> Build()
    {
        Environment.SpecialFolder[] folders =
        [
            Environment.SpecialFolder.Windows,
            Environment.SpecialFolder.System,
            Environment.SpecialFolder.SystemX86,
            Environment.SpecialFolder.ProgramFiles,
            Environment.SpecialFolder.ProgramFilesX86,
            Environment.SpecialFolder.CommonProgramFiles,
            Environment.SpecialFolder.CommonProgramFilesX86,
            Environment.SpecialFolder.CommonApplicationData,
            Environment.SpecialFolder.UserProfile,
        ];

        return KeysOf(folders.Select(Environment.GetFolderPath));
    }
}
