namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>
/// Which <c>HKEY_CLASSES_ROOT</c> keys contribute context-menu entries for a selection — the same
/// list Explorer's default menu builds, so an extension that shows up there shows up here.
/// </summary>
/// <remarks>
/// The first item decides the type-specific keys, as it does in Explorer: a mixed selection is
/// asked about as whatever came first, and the handlers themselves look at the whole selection
/// through the data object. <c>AllFilesystemObjects</c> is always last, because it is where the
/// extensions that apply to anything on disk register.
/// </remarks>
public static class ShellMenuKeys
{
    public const string Star = "*";
    public const string AllFileSystemObjects = "AllFilesystemObjects";
    public const string Directory = "Directory";
    public const string Folder = "Folder";
    public const string Drive = "Drive";
    public const string Background = @"Directory\Background";
    public const string SystemFileAssociations = "SystemFileAssociations";

    /// <summary>The keys that are not tied to one file type — what the Settings page scans so its
    /// list does not depend on what happens to be selected.</summary>
    public static IReadOnlyList<string> Generic { get; } =
        [Star, AllFileSystemObjects, Directory, Folder, Drive, Background];

    /// <param name="typeOf">What the registry says about an extension (".txt"), or null when it
    /// says nothing. Injected so this decides without a registry.</param>
    public static IReadOnlyList<string> For(
        IReadOnlyList<ShellMenuTarget> targets,
        ShellMenuContext context,
        Func<string, ShellFileType?> typeOf)
    {
        if (context == ShellMenuContext.Background) return [Background];
        if (targets.Count == 0) return [];

        var keys = new List<string>();
        var first = targets[0];

        if (first.IsDirectory)
        {
            keys.Add(IsDriveRoot(first.FullPath) ? Drive : Directory);
            keys.Add(Folder);
        }
        else
        {
            var extension = Path.GetExtension(first.FullPath);
            if (extension.Length > 1)
            {
                var type = typeOf(extension);
                if (type?.ProgId is { Length: > 0 } progId) keys.Add(progId);
                keys.Add(SystemFileAssociations + @"\" + extension);
                if (type?.PerceivedType is { Length: > 0 } perceived)
                    keys.Add(SystemFileAssociations + @"\" + perceived);
            }

            keys.Add(Star);
        }

        keys.Add(AllFileSystemObjects);
        return keys.Distinct(StringComparer.OrdinalIgnoreCase).ToList();
    }

    private static bool IsDriveRoot(string path)
    {
        var root = Path.GetPathRoot(path);
        return root is { Length: > 0 } &&
               string.Equals(
                   Path.TrimEndingDirectorySeparator(root),
                   Path.TrimEndingDirectorySeparator(path),
                   StringComparison.OrdinalIgnoreCase);
    }
}
