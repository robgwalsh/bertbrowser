using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.NewItem;

/// <summary>
/// The naming rule for a new folder or file: what the dialog suggests, what part of it is
/// pre-selected, and whether what the user typed can be a name at all.
/// </summary>
/// <remarks>
/// Pure, and separate from <see cref="NewItemPlanner"/>, for the reason <see cref="RenamePattern"/>
/// is separate from the rename planner: the dialog validates every keystroke, so the rule it
/// previews has to be the rule the create obeys rather than a second implementation that can drift.
/// The character and reserved-name rules are <see cref="RenamePattern.Validate"/>'s — a name
/// Windows refuses refuses it whether the file is arriving by rename or by creation.
/// </remarks>
public static class NewItemPattern
{
    /// <summary>Why <paramref name="name"/> can't be created here, or null when it can.</summary>
    /// <param name="name">What the user typed. For a file this includes any extension they typed
    /// themselves.</param>
    /// <param name="extension">An extension the chosen file type will append, or "" when it will
    /// not. Checked <em>with</em> the name rather than separately, because the length limit applies
    /// to what lands on disk and the box only holds half of it.</param>
    public static string? Validate(string name, string extension = "")
    {
        // Cleaned first, so the rule matches what a rename does and what Windows will actually
        // store: "Reports. " is a perfectly good request for a folder called "Reports".
        var cleaned = Clean(name);
        if (cleaned.Length == 0) return "Enter a name.";

        return RenamePattern.Validate(cleaned + extension);
    }

    /// <summary>The name the dialog opens with: <paramref name="baseName"/>, or the first
    /// "(2)"-style variant of it that is free, so a second New Folder in the same place does not
    /// open on a name that is already refused.</summary>
    public static string SuggestName(
        INewItemProbe probe,
        string directory,
        string baseName,
        NewItemKind kind,
        string extension = "")
    {
        try
        {
            var candidate = System.IO.Path.Combine(directory, baseName + extension);
            var free = UniquePath.For(
                candidate, kind == NewItemKind.Folder, probe.DirectoryExists, probe.FileExists);
            return System.IO.Path.GetFileName(free);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException
                                       or PathTooLongException)
        {
            // An unusable directory is the planner's refusal to report, not a reason to fail here.
            return baseName + extension;
        }
    }

    /// <summary>The length of the part of <paramref name="name"/> the dialog pre-selects, so typing
    /// over the suggestion replaces the stem and leaves the extension alone.</summary>
    public static int StemLength(string name, NewItemKind kind)
    {
        if (kind == NewItemKind.Folder) return name.Length;
        var stem = System.IO.Path.GetFileNameWithoutExtension(name);
        // A dotfile such as .gitignore is all extension as far as Path is concerned; select the lot.
        return stem.Length == 0 ? name.Length : stem.Length;
    }

    /// <summary>Trailing spaces and periods are silently dropped by Windows, so take them off
    /// before anything sees the name — the same cleaning a rename does.</summary>
    public static string Clean(string name) => name.Trim().TrimEnd('.', ' ');
}
