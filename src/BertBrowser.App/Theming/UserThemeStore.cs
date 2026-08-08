using System.IO;
using System.Text;
using BertBrowser.App.Services;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Theming;

/// <summary>
/// The user's own themes, one <c>*.json</c> file each in <see cref="AppPaths.ThemesDir"/>.
/// </summary>
/// <remarks>
/// Everything here is best-effort: a themes folder is something the user can edit, sync, or delete
/// out from under the app, so a bad file costs that one theme and nothing else.
/// </remarks>
public sealed class UserThemeStore
{
    public string Directory => AppPaths.ThemesDir;

    public string PathFor(string id) => Path.Combine(Directory, id + ".json");

    /// <summary>
    /// Reads every theme in the folder. Files that don't parse are reported and skipped rather than
    /// aborting the scan — one broken theme must not hide the rest.
    /// </summary>
    public IReadOnlyList<ThemeDefinition> Load(out IReadOnlyList<ThemeIssue> issues)
    {
        var found = new List<ThemeDefinition>();
        var problems = new List<ThemeIssue>();
        issues = problems;

        if (!System.IO.Directory.Exists(Directory)) return found;

        string[] files;
        try
        {
            files = System.IO.Directory.GetFiles(Directory, "*.json");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            problems.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                $"Your themes folder could not be read: {ex.Message}"));
            return found;
        }

        Array.Sort(files, StringComparer.OrdinalIgnoreCase);

        foreach (var file in files)
        {
            try
            {
                if (!ThemeJson.TryDeserialize(File.ReadAllText(file), out var theme, out var error))
                {
                    problems.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                        $"'{Path.GetFileName(file)}' is not a valid theme: {error}"));
                    continue;
                }

                if (ThemeCatalog.Find(theme!.Id) is not null)
                {
                    problems.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                        $"'{Path.GetFileName(file)}' uses the id of a built-in theme ('{theme.Id}') and was skipped."));
                    continue;
                }

                found.Add(theme);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                problems.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                    $"'{Path.GetFileName(file)}' could not be read: {ex.Message}"));
            }
        }

        return found;
    }

    public bool TrySave(ThemeDefinition definition, string json, out string? error)
    {
        try
        {
            System.IO.Directory.CreateDirectory(Directory);
            File.WriteAllText(PathFor(definition.Id), json, new UTF8Encoding(false));
            error = null;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            error = ex.Message;
            return false;
        }
    }

    public bool TryDelete(string id, out string? error)
    {
        try
        {
            var path = PathFor(id);
            if (File.Exists(path)) File.Delete(path);
            error = null;
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            error = ex.Message;
            return false;
        }
    }

    /// <summary>
    /// Turns a display name into a filename-safe id, then suffixes it until it is unique among
    /// <paramref name="taken"/> — so "My Theme" and "my theme!" can coexist.
    /// </summary>
    public static string UniqueId(string name, IEnumerable<string> taken)
    {
        var slug = new StringBuilder();
        foreach (var c in name.Trim().ToLowerInvariant())
        {
            if (char.IsLetterOrDigit(c)) slug.Append(c);
            else if (slug.Length > 0 && slug[^1] != '-') slug.Append('-');
        }

        var basis = slug.ToString().Trim('-');
        if (basis.Length == 0) basis = "theme";

        var used = new HashSet<string>(taken, StringComparer.OrdinalIgnoreCase);
        if (!used.Contains(basis)) return basis;

        for (var i = 2; ; i++)
        {
            var candidate = $"{basis}-{i}";
            if (!used.Contains(candidate)) return candidate;
        }
    }
}
