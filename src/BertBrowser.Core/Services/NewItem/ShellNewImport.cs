using BertBrowser.Core.Services.Rename;

namespace BertBrowser.Core.Services.NewItem;

/// <summary>How Windows says a new file of some type should be produced.</summary>
public enum ShellNewKind
{
    /// <summary>An empty file.</summary>
    NullFile,

    /// <summary>Copy a template file, named by <see cref="ShellNewEntry.FileName"/>.</summary>
    FileName,

    /// <summary>Write <see cref="ShellNewEntry.Data"/> as the file's contents.</summary>
    Data,

    /// <summary>Run a program to produce it. Never honoured — see <see cref="ShellNewImport"/>.</summary>
    Command,
}

/// <summary>
/// The raw ShellNew values for one extension, exactly as the registry holds them.
/// </summary>
/// <remarks>
/// Nothing here has been interpreted yet, which is the point: it is what lets the interpretation be
/// tested in a project that cannot read a registry.
/// </remarks>
/// <param name="Extension">Including the dot, e.g. ".txt".</param>
/// <param name="Label">The friendly type name from the extension's ProgID, which may still be an
/// unresolved indirect resource string, or null when there wasn't one.</param>
/// <param name="Kind">Which of the four value shapes this entry uses.</param>
/// <param name="FileName">The template named by a <see cref="ShellNewKind.FileName"/> entry.</param>
/// <param name="Data">The bytes held by a <see cref="ShellNewKind.Data"/> entry.</param>
public sealed record ShellNewEntry(
    string Extension,
    string? Label,
    ShellNewKind Kind,
    string? FileName = null,
    byte[]? Data = null);

/// <summary>
/// Turns what the registry says into the app's own list of new-file types.
/// </summary>
/// <remarks>
/// <para>
/// This is the half of the ShellNew read that has behaviour worth asserting, kept in Core so it can
/// be tested; the App half only opens keys and hands the values over.
/// </para>
/// <para>
/// <b><see cref="ShellNewKind.Command"/> entries are dropped.</b> They name a program to run — it is
/// how Shortcut and Briefcase work — and honouring one would put a registry-supplied command line
/// through <c>ProcessLauncher</c>, past the single chokepoint this app starts programs from. A
/// "New" menu is not worth that.
/// </para>
/// </remarks>
public static class ShellNewImport
{
    /// <summary>The types worth offering, in the order given.</summary>
    /// <param name="entries">What the registry held.</param>
    /// <param name="resolveIndirectString">Resolves an <c>@file,-id</c> label, or returns null when
    /// it cannot. Injected so the fallback is testable without a real resource DLL.</param>
    /// <param name="fileExists">Whether a template file is really there.</param>
    /// <param name="templateRoots">Folders a bare template name is resolved against.</param>
    /// <param name="saveData">Writes a <see cref="ShellNewKind.Data"/> entry's bytes out to a file
    /// and returns its path, or null if it could not. Keeps the file writing in the App while the
    /// decision to do it stays here.</param>
    public static IReadOnlyList<NewFileTemplate> ToTemplates(
        IEnumerable<ShellNewEntry> entries,
        Func<string, string?> resolveIndirectString,
        Func<string, bool> fileExists,
        IReadOnlyList<string> templateRoots,
        Func<string, byte[], string?> saveData)
    {
        var templates = new List<NewFileTemplate>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var entry in entries)
        {
            // A registry-supplied command line is never worth a menu entry.
            if (entry.Kind == ShellNewKind.Command) continue;

            if (!IsUsableExtension(entry.Extension)) continue;
            if (!seen.Add(entry.Extension)) continue;

            string? templatePath = null;
            switch (entry.Kind)
            {
                case ShellNewKind.FileName:
                    templatePath = ResolveTemplate(entry.FileName, templateRoots, fileExists);
                    // A type whose template is not installed would only ever refuse; leave it out.
                    if (templatePath is null) continue;
                    break;

                case ShellNewKind.Data:
                    if (entry.Data is not { Length: > 0 } data) break;
                    templatePath = saveData(entry.Extension, data);
                    if (templatePath is null) continue;
                    break;
            }

            templates.Add(new NewFileTemplate
            {
                Label = LabelFor(entry, resolveIndirectString),
                Extension = entry.Extension.ToLowerInvariant(),
                TemplatePath = templatePath,
            });
        }

        return templates;
    }

    /// <summary>What the menu should call this type.</summary>
    public static string LabelFor(ShellNewEntry entry, Func<string, string?> resolveIndirectString)
    {
        var label = entry.Label;

        if (label is { Length: > 0 } && IsIndirectString(label))
        {
            string? resolved = null;
            try { resolved = resolveIndirectString(label); }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { }

            // An unresolvable resource reference must never reach the menu as its own raw text.
            label = string.IsNullOrWhiteSpace(resolved) ? null : resolved;
        }

        return string.IsNullOrWhiteSpace(label) ? FallbackLabel(entry.Extension) : label.Trim();
    }

    /// <summary>Whether a label is a resource reference rather than real text.</summary>
    public static bool IsIndirectString(string label) => label.StartsWith('@');

    /// <summary>".txt" becomes "TXT File" — what Explorer shows for a type it has no name for.</summary>
    public static string FallbackLabel(string extension) =>
        $"{extension.TrimStart('.').ToUpperInvariant()} File";

    /// <summary>Where a ShellNew template really is, or null if it is nowhere. A bare name is
    /// resolved against the template folders; a rooted one is taken as given.</summary>
    public static string? ResolveTemplate(
        string? fileName, IReadOnlyList<string> roots, Func<string, bool> fileExists)
    {
        if (fileName is not { Length: > 0 }) return null;

        try
        {
            if (Path.IsPathRooted(fileName))
                return fileExists(fileName) ? fileName : null;

            foreach (var root in roots)
            {
                var candidate = Path.Combine(root, fileName);
                if (fileExists(candidate)) return candidate;
            }
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException
                                       or PathTooLongException)
        {
        }

        return null;
    }

    /// <summary>Existing entries keep their place and their settings; an import only adds types for
    /// extensions the user does not already have, so re-importing never disturbs the list.</summary>
    public static IReadOnlyList<NewFileTemplate> Merge(
        IReadOnlyList<NewFileTemplate> existing, IReadOnlyList<NewFileTemplate> imported)
    {
        var known = existing
            .Select(t => t.Extension)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        return [.. existing, .. imported.Where(t => known.Add(t.Extension))];
    }

    /// <summary>An extension has to be able to end a legal file name, or the type it names could
    /// never produce one.</summary>
    private static bool IsUsableExtension(string extension) =>
        extension.Length > 1
        && extension[0] == '.'
        && extension.IndexOf('.', 1) < 0
        && RenamePattern.Validate("x" + extension) is null;
}
