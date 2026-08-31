namespace BertBrowser.Core.Services;

/// <summary>
/// Turns the <c>code</c> launcher on <c>PATH</c> into the editor's own executable.
/// </summary>
/// <remarks>
/// <para>
/// <c>code</c> is not a program. It is <c>…\Microsoft VS Code\bin\code.cmd</c>, a batch file that
/// starts <c>Code.exe</c> as a Node process to talk to the running instance — so starting it
/// through the shell starts <b>cmd.exe</b>, and a console window sits on screen beside the editor
/// it opened for as long as that takes. It reads as the app having opened a terminal by mistake.
/// </para>
/// <para>
/// Nothing about the window is fixable at the launch. Hiding it means <c>UseShellExecute = false</c>,
/// which is the switch that makes file associations and the <c>runas</c> verb work, and
/// <c>ProcessStartInfo</c> offers no "no window" that applies to a shell execute. So the shim
/// is stepped over instead: the editor is started directly, which is also one fewer process in the
/// chain and one fewer thing between the click and the window.
/// </para>
/// <para>
/// Pure, and takes its probe as an argument, so the rule is unit-tested rather than inferred from
/// whichever editor happens to be installed on the machine running the tests. It refuses far more
/// than it accepts — anything that is not a <c>.cmd</c>/<c>.bat</c> in a <c>bin</c> folder with a
/// known editor beside it comes back null, and the caller keeps the launcher it already had.
/// </para>
/// </remarks>
public static class VSCodePath
{
    /// <summary>
    /// What an install root may call the editor, best first. Insiders and VSCodium are here because
    /// their launchers are the ones a stem-matching rule would miss: the shim is
    /// <c>code-insiders.cmd</c> and the program beside it is <c>Code - Insiders.exe</c>.
    /// </summary>
    public static readonly string[] Executables =
        ["Code.exe", "Code - Insiders.exe", "VSCodium.exe", "VSCodium - Insiders.exe"];

    /// <summary>
    /// The editor executable a resolved launcher stands in for, or null when it is not a shim of
    /// that shape or nothing beside it is an editor.
    /// </summary>
    /// <param name="launcher">An absolute path, as <see cref="ExecutablePath.Resolve"/> returns.</param>
    /// <param name="resolve">Probes for a file, answering with its absolute path or null.
    /// <see cref="ExecutablePath.Resolve"/> over a fully qualified path is exactly that.</param>
    public static string? BehindLauncher(string? launcher, Func<string, string?> resolve)
    {
        ArgumentNullException.ThrowIfNull(resolve);

        if (string.IsNullOrWhiteSpace(launcher)) return null;

        var extension = Path.GetExtension(launcher);
        if (!extension.Equals(".cmd", StringComparison.OrdinalIgnoreCase) &&
            !extension.Equals(".bat", StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        // The folder has to be a `bin`, and the editor is the level above it. Without that the rule
        // would be "run the .exe next to any .cmd", which is a different program than the one the
        // user asked for whenever the guess is wrong.
        var bin = Path.GetDirectoryName(launcher);
        if (bin is null || !Path.GetFileName(bin).Equals("bin", StringComparison.OrdinalIgnoreCase))
            return null;

        var root = Path.GetDirectoryName(bin);
        if (string.IsNullOrEmpty(root)) return null;

        foreach (var name in Executables)
        {
            string candidate;
            try
            {
                candidate = Path.Combine(root, name);
            }
            catch (ArgumentException)
            {
                continue;
            }

            if (resolve(candidate) is { } editor) return editor;
        }

        return null;
    }
}
