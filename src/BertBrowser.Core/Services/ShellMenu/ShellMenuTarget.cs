namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>One thing a shell context menu is asked about: a selected row, or the folder itself
/// when the right-click landed on empty space.</summary>
public sealed record ShellMenuTarget(string FullPath, bool IsDirectory);

/// <summary>What the menu is over. The shell registers extensions separately for "these items" and
/// for "this folder's background", and a Git Bash Here belongs to the second.</summary>
public enum ShellMenuContext
{
    Items,
    Background,
}

/// <summary>What the registry says about a file extension — the ProgID it maps to and its
/// perceived type — read by the App and handed in raw, so <see cref="ShellMenuKeys"/> decides
/// without opening a key.</summary>
public sealed record ShellFileType(string? ProgId, string? PerceivedType);
