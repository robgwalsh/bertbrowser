using System.Diagnostics;
using System.IO;
using System.Windows;
using BertBrowser.App.Interop;
using BertBrowser.App.Views;
using BertBrowser.Core.Services;

namespace BertBrowser.App.Services;

/// <summary>
/// The one way this app starts another program. Nothing else may call <see cref="Process.Start"/>:
/// this process holds an administrator token, and a child started directly inherits it silently.
/// </summary>
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
    /// chain ("Windows Terminal, else PowerShell") ask this first: a launch no longer throws when
    /// the program is missing, so this is how they find out.
    /// </summary>
    string? Resolve(string program);
}

/// <inheritdoc cref="IProcessLauncher"/>
/// <remarks>
/// Mechanism lives in <see cref="ShellLauncher"/>; this is the policy on top of it. The policy is
/// one sentence: <b>nothing starts elevated unless the user chose it.</b> So when the shell route
/// is unavailable — no explorer, an unusual shell, a shell that did not answer in time — this does
/// not quietly fall back to the elevated launch that was the original problem. It says so, and
/// asks.
/// </remarks>
public sealed class ProcessLauncher : IProcessLauncher
{
    public string? Launch(string file, string? arguments = null, string? workingDirectory = null,
        bool elevated = false)
    {
        if (string.IsNullOrWhiteSpace(file)) return "Nothing to open.";

        var verb = elevated ? "runas" : null;
        var result = ShellLauncher.ShellExecuteAsUser(file, arguments, workingDirectory, verb, out var error);

        return result switch
        {
            ShellLaunchResult.Launched => null,

            // The shell was reached and may still be working on it. Offering to start it another
            // way here is how you end up running it twice, so this only reports.
            ShellLaunchResult.Unresponsive =>
                $"Windows Explorer did not respond. '{Describe(file)}' may still open.",

            // No shell to hand it to, and doing it ourselves means doing it as administrator.
            // That is the user's call, not ours.
            _ => AskThenLaunchElevated(file, arguments, workingDirectory, error),
        };
    }

    public string? Resolve(string program) => ExecutablePath.Resolve(
        program,
        Environment.GetEnvironmentVariable("PATH"),
        Environment.GetEnvironmentVariable("PATHEXT"),
        File.Exists);

    /// <summary>What to call this file in a sentence.</summary>
    private static string Describe(string file) =>
        Path.GetFileName(file.TrimEnd('\\')) is { Length: > 0 } name ? name : file;

    private static string? AskThenLaunchElevated(
        string file, string? arguments, string? workingDirectory, string? error)
    {
        var name = Describe(file);
        var message =
            $"Windows could not be asked to open '{name}' as your normal user account " +
            $"({error?.TrimEnd('.') ?? "the shell did not respond"}).\n\n" +
            "BertBrowser runs as administrator so it can index your drives, so opening it from here " +
            "would run it as administrator too — with full access to this computer.\n\n" +
            "Open it as administrator anyway?";

        if (!Confirm(message)) return $"'{name}' was not opened.";

        try
        {
            Process.Start(new ProcessStartInfo(file, arguments ?? "")
            {
                UseShellExecute = true,
                WorkingDirectory = workingDirectory ?? "",
            });
            return null;
        }
        catch (Exception ex) when (ex is System.ComponentModel.Win32Exception or InvalidOperationException
                                      or FileNotFoundException)
        {
            return $"Cannot open: {ex.Message}";
        }
    }

    /// <summary>Dialogs belong to the UI thread, and a launch can be requested from a background
    /// continuation, so the ask is marshalled rather than assumed to be on it.</summary>
    private static bool Confirm(string message)
    {
        var app = Application.Current;
        if (app is null) return false;

        return app.Dispatcher.Invoke(() => MessageDialog.Show(
            app.MainWindow,
            message,
            "Run as administrator?",
            MessageDialogKind.Warning,
            showCancel: true));
    }
}
