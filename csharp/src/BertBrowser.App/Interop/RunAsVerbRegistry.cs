using BertBrowser.Core.Services.ShellIntegration;
using Microsoft.Win32;

namespace BertBrowser.App.Interop;

/// <summary>
/// Asks the registry whether a file type carries a <c>runas</c> verb — that is, whether "Run as
/// administrator" can do anything at all with it.
/// </summary>
/// <remarks>
/// <para>
/// <b>Read-only, like <see cref="ShellNewRegistry"/>.</b> Every key is opened without write access
/// and nothing here creates, sets or deletes a value. What counts as runnable is decided by
/// <see cref="RunAsVerbRules"/> in Core; this only reports what the machine says.
/// </para>
/// <para>
/// The lookup mirrors how the shell resolves a verb, and the order matters. The user's own choice of
/// program (<c>FileExts\.ext\UserChoice</c>) wins over the machine default, an extension key can
/// carry verbs directly, and <c>SystemFileAssociations</c> is where Windows hangs verbs that apply
/// to a whole family regardless of which program opens it. Miss any of them and the item is greyed
/// out on something that would have worked, which is a worse failure than the one being fixed.
/// </para>
/// <para>
/// Answers are cached per extension for the life of the process. The context menu asks on every
/// right-click, and a file type does not gain a <c>runas</c> verb while the menu is open.
/// </para>
/// </remarks>
internal static class RunAsVerbRegistry
{
    private static readonly Dictionary<string, bool?> Cache = new(StringComparer.Ordinal);
    private static readonly Lock Gate = new();

    private static readonly Dictionary<string, string?> Commands = new(StringComparer.Ordinal);

    /// <summary>How this item can be run as administrator, if at all.</summary>
    public static ElevatedOpen Decide(string path, bool isDirectory, bool insideArchive) =>
        RunAsVerbRules.Decide(
            path, isDirectory, insideArchive, HasRunAsVerb, OpenCommandFor, File.Exists);

    /// <summary>Whether this item can be run as administrator.</summary>
    public static bool CanRunElevated(string path, bool isDirectory, bool insideArchive) =>
        Decide(path, isDirectory, insideArchive).Kind != ElevatedOpenKind.None;

    /// <summary>
    /// The command Windows would run to open this type, with environment variables expanded — or
    /// null when there is none to be had.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Expanded here rather than in Core so the parser stays pure, and so <c>%SystemRoot%</c> is
    /// resolved before anything goes looking for <c>%1</c>.
    /// </para>
    /// <para>
    /// <b>A <c>DelegateExecute</c> command is refused.</b> Those are COM handlers — the command line
    /// beside them is a fallback the shell may ignore entirely, and packaged apps use them — so
    /// starting it directly would run something other than what a double-click runs.
    /// </para>
    /// </remarks>
    private static string? OpenCommandFor(string extension)
    {
        lock (Gate)
        {
            if (Commands.TryGetValue(extension, out var cached)) return cached;

            var command = QueryCommand(extension);
            Commands[extension] = command;
            return command;
        }
    }

    private static string? QueryCommand(string extension)
    {
        try
        {
            foreach (var progId in ProgIdsFor(extension))
            {
                using var key = Registry.ClassesRoot.OpenSubKey(
                    $@"{progId}\shell\open\command", writable: false);
                if (key is null) continue;

                if (key.GetValue("DelegateExecute") is string { Length: > 0 }) continue;
                if (key.GetValue(null) is not string { Length: > 0 } command) continue;

                return Environment.ExpandEnvironmentVariables(command);
            }

            return null;
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException
                                      or IOException or ArgumentException or ObjectDisposedException)
        {
            return null;
        }
    }

    /// <summary>True, false, or null when the registry could not be read — which the rules answer
    /// with their own well-known list rather than a refusal.</summary>
    private static bool? HasRunAsVerb(string extension)
    {
        lock (Gate)
        {
            if (Cache.TryGetValue(extension, out var cached)) return cached;

            var answer = Query(extension);
            Cache[extension] = answer;
            return answer;
        }
    }

    private static bool? Query(string extension)
    {
        try
        {
            // Verbs hung on the extension itself, and on the family Windows files it under.
            if (HasVerb(extension) || HasVerb($@"SystemFileAssociations\{extension}")) return true;

            foreach (var progId in ProgIdsFor(extension))
                if (HasVerb(progId))
                    return true;

            return false;
        }
        catch (Exception ex) when (ex is System.Security.SecurityException or UnauthorizedAccessException
                                      or IOException or ArgumentException or ObjectDisposedException)
        {
            // ArgumentException is not hypothetical: a registry key name caps at 255 characters and
            // a file name does not, so a file with a preposterous extension would otherwise throw
            // out of here — and this runs on every selection change, where a throw breaks selecting.
            // Could not tell. Deliberately not "false": a machine whose registry cannot be read
            // should still be able to run an .exe as administrator.
            return null;
        }
    }

    /// <summary>The classes to consider for an extension, most specific first: the user's own choice
    /// of program, then the machine default.</summary>
    private static IEnumerable<string> ProgIdsFor(string extension)
    {
        var chosen = UserChoice(extension);
        if (chosen is { Length: > 0 }) yield return chosen;

        using var key = Registry.ClassesRoot.OpenSubKey(extension, writable: false);
        if (key?.GetValue(null) as string is { Length: > 0 } progId && progId != chosen)
            yield return progId;
    }

    private static string? UserChoice(string extension)
    {
        using var key = Registry.CurrentUser.OpenSubKey(
            $@"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{extension}\UserChoice",
            writable: false);

        return key?.GetValue("ProgId") as string;
    }

    /// <summary>Whether a class has a <c>runas</c> verb. <c>ClassesRoot</c> rather than HKLM and
    /// HKCU separately, because that view is the merge the shell itself resolves against.</summary>
    private static bool HasVerb(string className)
    {
        using var key = Registry.ClassesRoot.OpenSubKey($@"{className}\shell\runas", writable: false);
        return key is not null;
    }
}
