using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using BertBrowser.Core.Services;

namespace BertBrowser.App.Services;

/// <summary>
/// The one way this app starts another program.
/// </summary>
/// <remarks>
/// Still a single chokepoint, though no longer for the original reason. This process used to hold
/// an administrator token, so a child started directly inherited it without a prompt — opening a
/// downloaded <c>.exe</c> ran it as administrator. The app is <c>asInvoker</c> now and a child
/// inherits an ordinary token, so the danger is gone; what remains is worth keeping anyway, because
/// one place that starts programs is one place to audit, to fake in the harness, and to change.
/// </remarks>
public interface IProcessLauncher
{
    /// <summary>
    /// Starts <paramref name="file"/> as the logged-on user, or — when <paramref name="elevated"/>
    /// is set — as administrator, with a UAC prompt.
    /// </summary>
    /// <returns>Null when it started; otherwise a message for the status bar.</returns>
    string? Launch(string file, string? arguments = null, string? workingDirectory = null,
        bool elevated = false);

    /// <summary>
    /// The absolute path <paramref name="program"/> resolves to, or null. Callers with a fallback
    /// chain ("Windows Terminal, else PowerShell") ask this first, and it also decides <em>what</em>
    /// runs here — against <c>PATH</c>, rather than leaving a bare name to be resolved later
    /// against whatever folder happens to be current.
    /// </summary>
    string? Resolve(string program);
}

/// <inheritdoc cref="IProcessLauncher"/>
/// <remarks>
/// <para>
/// The policy is still one sentence — <b>nothing starts elevated unless the user chose it</b> — but
/// it now costs nothing to keep. An ordinary launch is an ordinary launch; <c>runas</c> from a
/// medium-integrity process raises a real UAC prompt, which is what "Run as administrator" should
/// have meant all along. From the old elevated process the same verb elevated silently, since the
/// token was already there, which is why this used to be several hundred lines of COM reaching into
/// <c>explorer.exe</c> to borrow a lesser token back.
/// </para>
/// <para>
/// A declined prompt is <c>ERROR_CANCELLED</c> and is reported as the user's choice rather than as
/// a failure.
/// </para>
/// </remarks>
public sealed class ProcessLauncher : IProcessLauncher
{
    private const int ERROR_CANCELLED = 1223;

    public string? Launch(string file, string? arguments = null, string? workingDirectory = null,
        bool elevated = false)
    {
        if (string.IsNullOrWhiteSpace(file)) return "Nothing to open.";

        try
        {
            Process.Start(new ProcessStartInfo(file, arguments ?? "")
            {
                UseShellExecute = true,
                Verb = elevated ? "runas" : "",
                WorkingDirectory = workingDirectory ?? "",
            });
            return null;
        }
        catch (Win32Exception ex) when (ex.NativeErrorCode == ERROR_CANCELLED)
        {
            return $"'{Describe(file)}' was not opened.";
        }
        catch (Exception ex) when (ex is Win32Exception or InvalidOperationException
                                      or FileNotFoundException or ObjectDisposedException)
        {
            return $"Cannot open: {ex.Message}";
        }
    }

    public string? Resolve(string program) => ExecutablePath.Resolve(
        program,
        Environment.GetEnvironmentVariable("PATH"),
        Environment.GetEnvironmentVariable("PATHEXT"),
        File.Exists);

    /// <summary>What to call this file in a sentence.</summary>
    private static string Describe(string file) =>
        Path.GetFileName(file.TrimEnd('\\')) is { Length: > 0 } name ? name : file;
}
