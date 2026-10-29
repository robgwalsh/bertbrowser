namespace BertBrowser.Core.Models;

/// <summary>A user-pinned file or directory shown in the sidebar's Bookmarks section.</summary>
/// <param name="DisplayPath">Casing-preserving normalized path (from <c>PathKey.NormalizeDisplay</c>).</param>
public sealed record Bookmark(string DisplayPath, bool IsDirectory);
