using BertBrowser.Core.Layout;

namespace BertBrowser.Core.Models;

/// <summary>A pane arrangement stored under a user-chosen name, shown in the sidebar's Workspaces
/// section.</summary>
/// <param name="Name">The identity; unique ignoring case.</param>
/// <param name="Layout">The captured pane tree — splits, tabs, sort and columns — in the same
/// shape <see cref="SessionLayout"/> uses for the single unnamed session.</param>
public sealed record SavedWorkspace(string Name, SessionLayout Layout);
