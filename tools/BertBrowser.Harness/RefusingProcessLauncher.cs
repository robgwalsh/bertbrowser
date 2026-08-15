using BertBrowser.App.Services;

namespace BertBrowser.Harness;

/// <summary>
/// The <see cref="IProcessLauncher"/> a scripted run gets: one that starts nothing.
/// </summary>
/// <remarks>
/// <para>
/// Parking the window offscreen is only half of not interrupting someone. Opening a file in
/// BertBrowser starts whatever program owns it — a video player, a browser, an installer — and
/// that program's window belongs to the desktop, not to this harness. A script that double-clicked
/// a row would put a real application in front of whoever is at the keyboard.
/// </para>
/// <para>
/// Refusing rather than no-op'ing on purpose: <see cref="Launch"/> returns a status-bar message,
/// which is exactly the channel the real launcher uses to report that it could not start something,
/// so a script can assert on it. "Open in Terminal", "Open in VS Code", custom commands and the
/// portable-device handler all come through here too.
/// </para>
/// </remarks>
internal sealed class RefusingProcessLauncher : IProcessLauncher
{
    /// <summary>Everything this run was asked to start, newest last, so a script can check that a
    /// gesture reached the launcher at all.</summary>
    public List<string> Attempts { get; } = [];

    public string? Launch(string file, string? arguments = null, string? workingDirectory = null,
        bool elevated = false)
    {
        Attempts.Add(elevated ? $"{file} (elevated)" : file);

        return $"The harness does not start programs: '{file}' was not launched.";
    }

    /// <summary>
    /// Resolves as the real launcher does.
    /// </summary>
    /// <remarks>
    /// This one is honest rather than refusing, because it only looks a name up — and callers with
    /// a fallback chain ("Windows Terminal, else PowerShell") pick <em>what</em> to run from its
    /// answer. Faking a null here would send them down a different branch than the app takes.
    /// </remarks>
    public string? Resolve(string program) => BertBrowser.Core.Services.ExecutablePath.Resolve(
        program,
        Environment.GetEnvironmentVariable("PATH"),
        Environment.GetEnvironmentVariable("PATHEXT"),
        File.Exists);
}
