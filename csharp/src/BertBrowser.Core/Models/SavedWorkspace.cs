using BertBrowser.Core.Layout;

namespace BertBrowser.Core.Models;

/// <summary>A pane arrangement stored under a user-chosen name, shown in the sidebar's Workspaces
/// section (or the title bar's dropdown) and listed in full on the Workspaces page of Settings.</summary>
/// <param name="Name">The identity; unique ignoring case.</param>
/// <param name="Layout">The captured pane tree — splits, tabs, sort and columns — in the same
/// shape <see cref="SessionLayout"/> uses for the single unnamed session.</param>
/// <param name="CreatedUtc">When it was first saved. Null only for one not yet stored.</param>
/// <param name="LastUsedUtc">When it was last switched to; null if it never has been.</param>
public sealed record SavedWorkspace(
    string Name,
    SessionLayout Layout,
    DateTime? CreatedUtc = null,
    DateTime? LastUsedUtc = null);
