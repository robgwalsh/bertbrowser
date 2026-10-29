using BertBrowser.Core.Services.ShellIntegration;

namespace BertBrowser.Core.Services.ShellMenu;

/// <summary>
/// The program to start for a static verb, for one target.
/// </summary>
/// <remarks>
/// <para>
/// Reuses <see cref="ShellOpenCommandParser"/>, which already splits a registered command into its
/// program and arguments and substitutes the shell's placeholders — <c>%1</c>, <c>%L</c>,
/// <c>%V</c> and friends all mean "the thing clicked", which is the item for an item verb and the
/// folder for a background one. <c>%W</c> is the one placeholder that means something else (the
/// working directory) and is substituted here first, so the parser's own rules stay as its other
/// caller, the run-as-administrator path, relies on them.
/// </para>
/// <para>
/// Unlike that caller, a verb with no placeholder at all is still run: a "Git Bash Here" that
/// takes its folder from the working directory is a legitimate registration, and there is no
/// token in hand to be careful with.
/// </para>
/// </remarks>
public static class StaticVerbCommand
{
    /// <param name="command">The verb's registered command line, environment variables already
    /// expanded.</param>
    /// <param name="targetPath">The item, or the folder for a background verb.</param>
    /// <param name="workingDirectory">The folder the menu was over.</param>
    /// <param name="exists">Whether a path names a file — injected so this is testable without a
    /// disk, as the parser's own is.</param>
    public static ShellOpenCommand? Resolve(
        string? command, string targetPath, string workingDirectory, Func<string, bool> exists)
    {
        if (string.IsNullOrWhiteSpace(command)) return null;

        var expanded = command
            .Replace("%W", workingDirectory, StringComparison.Ordinal)
            .Replace("%w", workingDirectory, StringComparison.Ordinal);

        return ShellOpenCommandParser.Parse(expanded, targetPath, exists, requirePlaceholder: false);
    }

    /// <summary>Where a verb runs: the folder for a background verb or a folder item, and the
    /// item's own folder otherwise.</summary>
    public static string WorkingDirectoryFor(ShellMenuTarget target, ShellMenuContext context, string folder) =>
        context == ShellMenuContext.Background || target.IsDirectory
            ? target.FullPath
            : Path.GetDirectoryName(target.FullPath) ?? folder;
}
