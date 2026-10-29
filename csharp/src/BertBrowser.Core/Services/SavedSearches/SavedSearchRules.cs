using BertBrowser.Core.Models;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.Search;

namespace BertBrowser.Core.Services.SavedSearches;

/// <summary>What running a saved search means: go to <paramref name="NavigateTo"/> first, then
/// put <paramref name="Text"/> in the whole-PC box (<paramref name="Global"/>) or the folder box.
/// Always "navigate, then type", even when the folder is the one already showing — the tab
/// short-circuits a same-path navigation, and one code path is one thing to get right.</summary>
public readonly record struct SavedSearchRun(string NavigateTo, string Text, bool Global);

/// <summary>
/// The decisions behind a saved search — whether one may be saved, what to call it by default,
/// and what running it means — kept pure so the dialog and the shell obey the same rules the
/// tests pin.
/// </summary>
public static class SavedSearchRules
{
    public const int MaxDefaultNameLength = 40;

    /// <summary>
    /// The first reason the search cannot be saved, in words for the user, or null when it can.
    /// </summary>
    /// <param name="nameTaken">Whether another saved search already has this (trimmed) name. The
    /// caller excludes the search being edited, so keeping its own name is not a clash.</param>
    /// <param name="isArchiveFile">Whether a path is an archive file on disk — the probe
    /// <see cref="ArchivePath.Parse"/> asks, so that a real folder called <c>x.zip</c> can still
    /// be pinned while a folder <em>inside</em> an archive cannot.</param>
    public static string? Validate(
        string name,
        string query,
        SavedSearchScope scope,
        string? scopePath,
        Func<string, bool> nameTaken,
        Func<string, bool> isArchiveFile)
    {
        var trimmedName = name.Trim();
        if (trimmedName.Length == 0) return "Give the search a name.";
        if (nameTaken(trimmedName)) return $"There is already a saved search called \"{trimmedName}\".";

        // Parse returns neither a query nor a problem for blank text and for text too broad to
        // run, so blank is told apart first; the remaining "neither" is the too-broad case, which
        // as a saved search would show the folder listing and look like a bug.
        if (string.IsNullOrWhiteSpace(query)) return "Type a search to save.";
        var parsed = SearchGrammar.Parse(query);
        if (parsed.Problem is not null) return parsed.Problem;
        if (parsed.Query is null) return "Too broad to save: type at least two characters, or a filter such as ext:jpg.";

        if (scope == SavedSearchScope.Folder)
        {
            if (string.IsNullOrWhiteSpace(scopePath)) return "Choose the folder to search in.";
            if (ArchivePath.Parse(scopePath, isArchiveFile) is not null)
                return "A folder inside an archive can't be pinned.";
        }
        else if (scopePath is not null)
        {
            return "Only a pinned search has a folder.";
        }

        return null;
    }

    /// <summary>The name a new saved search starts with: the query itself, tidied and capped, so
    /// the common case is pressing Enter.</summary>
    public static string DefaultName(string query)
    {
        var collapsed = string.Join(' ', query.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
        if (collapsed.Length == 0) return "Saved search";
        return collapsed.Length <= MaxDefaultNameLength
            ? collapsed
            : collapsed[..(MaxDefaultNameLength - 1)].TrimEnd() + "…";
    }

    /// <summary>
    /// Where and how to run <paramref name="search"/> from a tab showing
    /// <paramref name="currentPath"/>, or null when there is nowhere to run it: a tab with no
    /// folder cannot run any search, even a whole-PC one, because the search runs inside a
    /// listing and an empty tab has none.
    /// </summary>
    public static SavedSearchRun? Plan(SavedSearch search, string currentPath)
    {
        switch (search.Scope)
        {
            case SavedSearchScope.Folder when !string.IsNullOrEmpty(search.ScopePath):
                return new SavedSearchRun(search.ScopePath, search.Query, Global: false);
            case SavedSearchScope.CurrentFolder when currentPath.Length > 0:
                return new SavedSearchRun(currentPath, search.Query, Global: false);
            case SavedSearchScope.ThisPc when currentPath.Length > 0:
                return new SavedSearchRun(currentPath, search.Query, Global: true);
            default:
                return null;
        }
    }
}
