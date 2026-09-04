namespace BertBrowser.Core.Models;

/// <summary>Where a saved search runs when it is opened.</summary>
public enum SavedSearchScope
{
    /// <summary>A template: the folder the tab is showing at the time, whatever that is.</summary>
    CurrentFolder = 0,

    /// <summary>Pinned: always <see cref="SavedSearch.ScopePath"/>, navigating there first.</summary>
    Folder = 1,

    /// <summary>The whole-PC (index-backed) search.</summary>
    ThisPc = 2,
}

/// <summary>A search query stored under a user-chosen name, shown in the sidebar's Saved
/// searches section.</summary>
/// <param name="Name">The identity; unique ignoring case.</param>
/// <param name="Query">Text in the search language, exactly as it would be typed.</param>
/// <param name="ScopePath">The pinned folder, non-null iff <paramref name="Scope"/> is
/// <see cref="SavedSearchScope.Folder"/>. A casing-preserving display path, not a path key: this
/// table is keyed by name and never range-scanned, and navigation wants the real casing.</param>
public sealed record SavedSearch(string Name, string Query, SavedSearchScope Scope, string? ScopePath);
