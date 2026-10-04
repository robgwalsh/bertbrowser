using System.Diagnostics;
using System.Text.RegularExpressions;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Infrastructure;

/// <summary>
/// What was true of the machine and the tree when a run happened. Recorded, never acted on — the
/// one exception is <c>startup</c> refusing to run beside a real BertBrowser, and that is its own
/// check.
/// </summary>
internal static partial class EnvironmentProbe
{
    public static MachineInfo Machine(string? keyOverride)
    {
        if (!OperatingSystem.IsWindows())
            throw new InvalidOperationException("The benchmark tool measures a Windows application and runs on Windows.");

        var facts = MachineKey.Probe();
        return new MachineInfo(
            keyOverride ?? MachineKey.Compute(facts),
            facts.Cpu,
            facts.LogicalCores,
            facts.RamGb,
            facts.OsBuild,
            Environment.Version.ToString(),
            PowerPlan(),
            IsRunning("BertBrowser"),
            IsRunning("BertBrowser.Indexer"));
    }

    public static GitInfo Git(string repoRoot)
    {
        var sha = Run("git", "rev-parse HEAD", repoRoot) ?? "unknown";
        var branch = Run("git", "rev-parse --abbrev-ref HEAD", repoRoot) ?? "unknown";
        var status = Run("git", "status --porcelain", repoRoot);
        return new GitInfo(sha, branch, Dirty: status is null || status.Length > 0);
    }

    public static bool IsRunning(string processName)
    {
        var processes = Process.GetProcessesByName(processName);
        try
        {
            return processes.Length > 0;
        }
        finally
        {
            foreach (var p in processes) p.Dispose();
        }
    }

    private static string? PowerPlan()
    {
        // "Power Scheme GUID: 381b4222-...  (Balanced)" — the name is the parenthesised part.
        var text = Run("powercfg", "/getactivescheme", null);
        if (text is null) return null;
        var match = SchemeName().Match(text);
        return match.Success ? match.Groups[1].Value : null;
    }

    /// <summary>Runs a tool for its stdout, or null if it is missing, fails or hangs.</summary>
    private static string? Run(string file, string arguments, string? workingDirectory)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo(file, arguments)
            {
                WorkingDirectory = workingDirectory ?? "",
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true,
            });
            if (process is null) return null;

            var output = process.StandardOutput.ReadToEnd();
            if (!process.WaitForExit(10_000))
            {
                process.Kill(entireProcessTree: true);
                return null;
            }

            return process.ExitCode == 0 ? output.Trim() : null;
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or InvalidOperationException or IOException)
        {
            return null;
        }
    }

    [GeneratedRegex(@"\(([^)]+)\)\s*$")]
    private static partial Regex SchemeName();
}
