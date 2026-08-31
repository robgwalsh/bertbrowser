namespace BertBrowser.Core.Services.ShellIntegration;

/// <summary>
/// Whether "Run as administrator" can mean anything for a given file.
/// </summary>
/// <remarks>
/// <para>
/// <b><c>runas</c> is a verb registered per file type, not a thing you can do to any file.</b>
/// <c>exefile</c> has one; <c>txtfile</c> does not, and nothing will ever give it one. Asking the
/// shell to <c>runas</c> a <c>.txt</c> comes back as <c>ERROR_NO_ASSOCIATION</c> (1155), "No
/// application is associated with the specified file for this operation" — which is a baffling thing
/// to read about a file that opens perfectly well on a double-click, and until this existed it was
/// the only feedback the app gave.
/// </para>
/// <para>
/// Explorer answers this by greying the item out on anything that is not a program. This goes one
/// step further — see <see cref="Decide"/>: where there is no verb but the file has a handler, the
/// <em>handler</em> is started elevated with the file as its argument, which is what "run this as
/// administrator" means for a <c>.sln</c> or a config file. The item is greyed only when there is
/// nothing at all to start.
/// </para>
/// <para>
/// The decision is split the way <c>ShellNewImport</c>/<c>ShellNewRegistry</c> are: <b>this decides
/// and the App reads the registry</b>, so what counts as runnable is testable in a project that
/// cannot open a registry key.
/// </para>
/// <para>
/// <b>The registry is asked rather than an extension list consulted</b>, because a hard-coded list
/// is wrong for every file type an installed program registers a <c>runas</c> verb for, and greying
/// the item out on something that would have worked is a worse failure than the one being fixed. The
/// list below is only the fallback for when the registry cannot be read at all.
/// </para>
/// </remarks>
public static class RunAsVerbRules
{
    /// <summary>
    /// The types measured to carry a <c>runas</c> verb, used only when the registry could not be
    /// read at all.
    /// </summary>
    /// <remarks>
    /// Short, and checked rather than guessed. <c>.com</c>, <c>.msi</c>, <c>.ps1</c>, <c>.vbs</c>
    /// and <c>.scr</c> all look like they belong here and none of them does — <c>comfile</c>,
    /// <c>Msi.Package</c> and the rest carry no <c>runas</c> key, so Windows refuses them exactly as
    /// it refuses a text file. Listing one would put the original bug back on a machine whose
    /// registry cannot be read.
    /// </remarks>
    public static readonly IReadOnlySet<string> WellKnown = new HashSet<string>(StringComparer.Ordinal)
    {
        ".exe", ".bat", ".cmd", ".msc",
    };

    /// <summary>
    /// A shortcut, which the registry cannot answer for.
    /// </summary>
    /// <remarks>
    /// <b>The one place the registry is not the authority.</b> A <c>.lnk</c> carries no verbs of its
    /// own — the shell resolves it and applies the <em>target's</em> — and that resolution is not
    /// expressed as a <c>lnkfile\shell\runas</c> key. So the registry says "no" about a shortcut to a
    /// program that Windows will happily elevate. Measured both ways: <c>runas</c> on a shortcut to
    /// <c>notepad.exe</c> starts an elevated Notepad, and on a shortcut to a <c>.txt</c> it comes back
    /// as <c>ERROR_NO_ASSOCIATION</c> — which is the friendly message, not a silence. So it is offered
    /// and the shell decides, which is what Explorer does too.
    /// </remarks>
    private const string Shortcut = ".lnk";

    /// <summary>
    /// The extension to ask about, lower-cased and with its dot, or the empty string when there is
    /// none to ask about.
    /// </summary>
    /// <remarks>
    /// A dotfile is a name, not an extension: <c>.gitignore</c> has no type and
    /// <see cref="Path.GetExtension(string)"/> would happily call it one. An extension-less file has
    /// no class either, so neither can carry a verb.
    /// </remarks>
    public static string ExtensionOf(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return "";

        var name = Path.GetFileName(path.TrimEnd('\\', '/'));
        var dot = name.LastIndexOf('.');
        if (dot <= 0 || dot == name.Length - 1) return "";

        return name[dot..].ToLowerInvariant();
    }

    /// <summary>
    /// Whether the item may be run as administrator at all.
    /// </summary>
    /// <param name="probe">Asks the registry whether an extension's class carries a <c>runas</c>
    /// verb. Null means it could not tell, which falls back to <see cref="WellKnown"/> rather than
    /// to a refusal — being unable to read the registry must not take the feature away from an
    /// <c>.exe</c>.</param>
    public static bool CanRunElevated(
        string path, bool isDirectory, bool insideArchive, Func<string, bool?> probe) =>
        Decide(path, isDirectory, insideArchive, probe, _ => null, _ => false).Kind != ElevatedOpenKind.None;

    /// <summary>
    /// How to run this item as administrator, if at all.
    /// </summary>
    /// <remarks>
    /// <para>
    /// Two ways, and the second is what makes the feature worth having. A handful of types carry a
    /// <c>runas</c> verb and the shell knows what to do with them. Everything else — a <c>.sln</c>, a
    /// <c>.docx</c>, a config file — has no such verb, but it does have an <em>open command</em>, and
    /// starting that program elevated with the file as its argument is exactly what "run this as
    /// administrator" means for a document.
    /// </para>
    /// <para>
    /// <b>This goes further than Explorer, on purpose.</b> Explorer greys the item out on a
    /// <c>.sln</c>; a file manager that can open a protected config in its own editor, elevated, is
    /// more useful than one that cannot, and the prompt is still the gate. What it will not do is
    /// guess: a handler it cannot resolve to a real program, or one that never says where the file
    /// goes, leaves the item greyed rather than starting something approximate with a token.
    /// </para>
    /// </remarks>
    /// <param name="openCommand">The registered open command for an extension, environment variables
    /// already expanded, or null when there is none to be had.</param>
    public static ElevatedOpen Decide(
        string path,
        bool isDirectory,
        bool insideArchive,
        Func<string, bool?> hasVerb,
        Func<string, string?> openCommand,
        Func<string, bool> exists)
    {
        // A folder has no program to run, and nothing inside a container has a path another process
        // could be pointed at — it has to be extracted first, which is what Open already says.
        if (isDirectory || insideArchive) return ElevatedOpen.None;

        var extension = ExtensionOf(path);
        if (extension.Length == 0) return ElevatedOpen.None;
        if (extension == Shortcut) return ElevatedOpen.Verb;

        if (hasVerb(extension) ?? WellKnown.Contains(extension)) return ElevatedOpen.Verb;

        return ShellOpenCommandParser.Parse(openCommand(extension), path, exists) is { } handler
            ? ElevatedOpen.Handler(handler.Executable, handler.Arguments)
            : ElevatedOpen.None;
    }

    /// <summary>What to say when it was asked for anyway — through a keyboard shortcut, or a custom
    /// command with the elevated box ticked. Windows' own wording for this is
    /// <c>ERROR_NO_ASSOCIATION</c>, which describes the mechanism rather than the problem.</summary>
    public static string CannotRunMessage(string name) =>
        $"There is no program Windows can run as administrator for '{name}'.";
}

/// <summary>Which of the two ways of elevating applies, if either.</summary>
public enum ElevatedOpenKind
{
    /// <summary>Nothing to do: no verb, and no handler worth guessing at.</summary>
    None,

    /// <summary>The shell has a <c>runas</c> verb for this type; hand it the file and let it work.</summary>
    Verb,

    /// <summary>No verb, but the file's handler can be started elevated with the file as its
    /// argument.</summary>
    Handler,
}

/// <param name="Executable">The program to start, for <see cref="ElevatedOpenKind.Handler"/>.</param>
/// <param name="Arguments">Its arguments, with the file already substituted in.</param>
public sealed record ElevatedOpen(ElevatedOpenKind Kind, string Executable = "", string Arguments = "")
{
    public static readonly ElevatedOpen None = new(ElevatedOpenKind.None);
    public static readonly ElevatedOpen Verb = new(ElevatedOpenKind.Verb);

    public static ElevatedOpen Handler(string executable, string arguments) =>
        new(ElevatedOpenKind.Handler, executable, arguments);
}
