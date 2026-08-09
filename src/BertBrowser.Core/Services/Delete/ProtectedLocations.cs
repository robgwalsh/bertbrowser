using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Delete;

/// <summary>
/// The handful of folders this app will not delete however they were selected. Ordinary Windows
/// stops most of these itself by refusing the access — but this app asks for elevation so it can
/// read the MFT, so that backstop is not there, and a stray Ctrl+A in the wrong folder would find
/// nothing in its way.
/// </summary>
/// <remarks>
/// Only the folders themselves are protected, not what is inside them: the point is to refuse the
/// one selection that ends the machine, not to make the app decline to manage files. Drive and
/// volume roots are refused separately by the planner, which is what makes exact matching enough
/// here — every entry below sits directly under a root that is already unreachable.
/// </remarks>
public static class ProtectedLocations
{
    /// <summary>Canonical keys of the locations refused by default.</summary>
    public static IReadOnlyCollection<string> Default { get; } = Build();

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
