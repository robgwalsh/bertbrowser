using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.ShellIntegration;

/// <summary>What the registry currently says about who opens folders and drives.</summary>
public enum FolderHandlerState
{
    /// <summary>Nothing of ours is there; the shell inherits Explorer from the <c>Folder</c> class.</summary>
    NotRegistered,

    /// <summary>Registered, complete, and naming the executable that is asking.</summary>
    RegisteredToThisApp,

    /// <summary>Ours, but wrong — a stale executable path, a half-written pair, or the default verb
    /// no longer pointing at it. This is the only state the startup self-heal may repair.</summary>
    RegisteredToThisAppStale,

    /// <summary>Some other program owns the verb. Never overwritten without being asked.</summary>
    RegisteredToAnotherApp,
}

/// <summary>The raw values a reader found. Nothing has been interpreted yet, which is the point:
/// it is what lets the interpretation be tested in a project that cannot open a registry key.</summary>
/// <param name="DirectoryDefaultVerb">
/// <c>(Default)</c> on <c>Directory\shell</c>. Null when HKCU has no value of its own — in which
/// case the HKLM <c>"none"</c> shows through and no verb is the default action.
/// </param>
public sealed record FolderHandlerReading(
    string? DirectoryDefaultVerb,
    string? DirectoryCommand,
    string? DriveDefaultVerb,
    string? DriveCommand)
{
    public static FolderHandlerReading None { get; } = new(null, null, null, null);
}

/// <summary>
/// Decides what a <see cref="FolderHandlerReading"/> means. Pure, so the self-heal rule, the
/// settings toggle's checked state and the "another program owns this" message all read from one
/// classification a test can hold still.
/// </summary>
public static class FolderHandlerRules
{
    /// <summary>
    /// Classifies a reading against the executable that would be registered.
    /// </summary>
    /// <remarks>
    /// The order of the checks is the design. <see cref="FolderHandlerState.RegisteredToAnotherApp"/>
    /// is decided first and wins outright, because every caller treats it as "do not touch" — a
    /// partial reading where someone else holds one of the two verbs must not be repaired into ours.
    /// Everything after that is ours, and the only question left is whether it is intact.
    /// </remarks>
    public static FolderHandlerState Classify(FolderHandlerReading reading, string executablePath)
    {
        ArgumentNullException.ThrowIfNull(reading);

        var directory = ProgramIn(reading.DirectoryCommand);
        var drive = ProgramIn(reading.DriveCommand);

        // Both shell keys must name the verb as their default action, or the shell never invokes it
        // — with the stock "none" in place it uses its own folder navigation instead and a perfect
        // command sits there unread.
        var directoryVerb = IsOpenVerb(reading.DirectoryDefaultVerb);
        var driveVerb = IsOpenVerb(reading.DriveDefaultVerb);

        // The dangerous shape, and the reason this is not folded into `intact` below: a default verb
        // named with no command behind it is what sends a folder double-click to a third party.
        // Reported as ours-and-broken so the toggle shows something to switch off, and switching it
        // off clears the value.
        if (directory is null && drive is null)
            return directoryVerb || driveVerb
                ? FolderHandlerState.RegisteredToThisAppStale
                : FolderHandlerState.NotRegistered;

        if (IsForeign(directory) || IsForeign(drive))
            return FolderHandlerState.RegisteredToAnotherApp;

        var intact =
            directory is not null && drive is not null &&
            SamePath(directory, executablePath) && SamePath(drive, executablePath) &&
            CurrentArguments(reading.DirectoryCommand) && CurrentArguments(reading.DriveCommand) &&
            directoryVerb && driveVerb;

        return intact ? FolderHandlerState.RegisteredToThisApp : FolderHandlerState.RegisteredToThisAppStale;
    }

    /// <summary>
    /// Whether the startup repair should rewrite the registration.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The failure this exists for is the one that matters: a registration naming an executable
    /// that is no longer there makes <b>every folder double-click fail</b>, with no way back except
    /// the registry. The Files app has a bug of exactly this shape on record (an antivirus
    /// interrupted its registry write and left the shell pointing at nothing), so the repair is
    /// worth having even though the uninstall hook is meant to prevent it.
    /// </para>
    /// <para>
    /// It is deliberately narrow. A registration that is <i>absent</i> is never created — the user
    /// may have removed it outside this app, and starting the app is not asking for it back. One
    /// belonging to another program is never touched. And a stale path whose executable still
    /// exists is left alone too, which is what stops a debug build run beside a real install from
    /// quietly repointing the shell at <c>bin\Debug</c>: the only stale registrations repaired are
    /// ones pointing at something gone, or already pointing here and merely incomplete.
    /// </para>
    /// </remarks>
    /// <param name="exists">Probes for a file. Injected so the rule is testable without one.</param>
    public static bool ShouldRepair(FolderHandlerReading reading, string executablePath, Func<string, bool> exists)
    {
        ArgumentNullException.ThrowIfNull(exists);

        if (Classify(reading, executablePath) != FolderHandlerState.RegisteredToThisAppStale)
            return false;

        var registered = RegisteredProgram(reading);
        if (registered is null) return false;

        return !exists(registered) || SamePath(registered, executablePath);
    }

    /// <summary>
    /// The program a registered command names, or null if nothing is registered. Used for the
    /// message naming another owner, and for asking whether a stale registration points at an
    /// executable that is still on disk.
    /// </summary>
    public static string? RegisteredProgram(FolderHandlerReading reading)
    {
        ArgumentNullException.ThrowIfNull(reading);
        return ProgramIn(reading.DirectoryCommand) ?? ProgramIn(reading.DriveCommand);
    }

    /// <summary>
    /// The program token of a shell command line: the quoted run if there is one, otherwise
    /// everything up to the first space. Null for a blank command or an unterminated quote.
    /// </summary>
    /// <remarks>
    /// This is not a general command-line parser and does not need to be. It reads back only what
    /// <see cref="FolderHandlerRegistration.CommandFor"/> writes, plus whatever another file
    /// manager wrote — and for that one, all that is wanted is a name to show and the knowledge
    /// that it is not ours.
    /// </remarks>
    public static string? ProgramIn(string? command)
    {
        if (string.IsNullOrWhiteSpace(command)) return null;

        var text = command.Trim();
        if (text[0] == '"')
        {
            var end = text.IndexOf('"', 1);
            return end > 1 ? text[1..end] : null;
        }

        var space = text.IndexOf(' ');
        var program = space < 0 ? text : text[..space];
        return program.Length == 0 ? null : program;
    }

    /// <summary>Whether a program token names this app, by file name — the path may be stale, and
    /// telling "ours, moved" from "somebody else's" is exactly what the caller needs.</summary>
    public static bool IsThisApp(string? program) =>
        program is not null &&
        string.Equals(
            SafeFileName(program),
            FolderHandlerRegistration.ExecutableName,
            StringComparison.OrdinalIgnoreCase);

    private static bool IsForeign(string? program) => program is not null && !IsThisApp(program);

    /// <summary>
    /// Whether a registered command carries the arguments this version writes. A build that changed
    /// them — adding <c>--new-tab</c>, say — leaves behind a registration that still launches the
    /// app and no longer does what it now means, which nothing would notice if only the program
    /// were compared. Reported as stale so the startup repair rewrites it.
    /// </summary>
    private static bool CurrentArguments(string? command) =>
        string.Equals(
            ArgumentsIn(command),
            FolderHandlerRegistration.ArgumentTail,
            StringComparison.OrdinalIgnoreCase);

    /// <summary>Everything after the program token, or "" when there is nothing.</summary>
    public static string ArgumentsIn(string? command)
    {
        if (string.IsNullOrWhiteSpace(command)) return "";

        var text = command.Trim();
        var after = text[0] == '"' ? text.IndexOf('"', 1) + 1 : text.IndexOf(' ');
        return after <= 0 || after >= text.Length ? "" : text[after..].Trim();
    }

    private static bool IsOpenVerb(string? verb) =>
        string.Equals(verb?.Trim(), FolderHandlerRegistration.OpenVerb, StringComparison.OrdinalIgnoreCase);

    /// <summary>Path comparison through <see cref="PathKey"/> rather than string equality, so a
    /// registration differing only in casing is recognised as already correct and the self-heal
    /// does not rewrite it on every launch.</summary>
    private static bool SamePath(string? a, string? b)
    {
        if (a is null || b is null) return false;
        try
        {
            return PathKey.Canonicalize(a) == PathKey.Canonicalize(b);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            return false;
        }
    }

    private static string? SafeFileName(string program)
    {
        try
        {
            return Path.GetFileName(program);
        }
        catch (ArgumentException)
        {
            return null;
        }
    }
}
