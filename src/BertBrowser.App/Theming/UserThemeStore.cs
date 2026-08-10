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

    /// <summary>
    /// The file a theme lives in. Refuses anything <see cref="ThemeId.IsSafe"/> rejects rather than
    /// letting <see cref="Path.Combine(string, string)"/> quietly resolve it somewhere else — see
    /// <see cref="ThemeId"/> for why that matters in an elevated process. Callers that handle
    /// untrusted ids check first; reaching here with a bad one is a bug.
    /// </summary>
    public string PathFor(string id)
    {
        if (!ThemeId.IsSafe(id))
            throw new ArgumentException($"'{id}' is not a usable theme id.", nameof(id));

        return Path.Combine(Directory, id + ".json");
    }

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

                // An id is a filename, so a theme dropped into the folder by hand could carry one
                // that points outside it — refuse it here rather than let a later save or delete
                // act on it. See ThemeId.
                if (!ThemeId.IsSafe(theme.Id))
                {
                    problems.Add(new ThemeIssue(ThemeIssueSeverity.Warning, null,
                        $"'{Path.GetFileName(file)}' has an unusable id ('{theme.Id}') and was skipped."));
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
        if (!ThemeId.IsSafe(definition.Id))
        {
            error = $"'{definition.Id}' is not a usable theme id.";
            return false;
        }

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
        if (!ThemeId.IsSafe(id))
        {
            error = $"'{id}' is not a usable theme id.";
            return false;
        }

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
}
