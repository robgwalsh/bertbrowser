namespace BertBrowser.Bench.Cli;

/// <summary>
/// Where the repository is, so every default path (<c>bench/baselines</c>, <c>bench-results</c>,
/// <c>docs/performance.md</c>) resolves the same way from any working directory.
/// </summary>
internal static class RepoRoot
{
    private const string Marker = "bertbrowser.sln";

    public static string Find(string? overridePath)
    {
        if (overridePath is not null)
        {
            var full = Path.GetFullPath(overridePath);
            return File.Exists(Path.Combine(full, Marker))
                ? full
                : throw new UsageException($"--repo: no {Marker} in '{full}'.");
        }

        return WalkUp(AppContext.BaseDirectory) ?? WalkUp(Directory.GetCurrentDirectory())
            ?? throw new InvalidOperationException(
                $"Could not find {Marker} above the executable or the current directory; pass --repo <dir>.");
    }

    private static string? WalkUp(string start)
    {
        for (var dir = new DirectoryInfo(start); dir is not null; dir = dir.Parent)
        {
            if (File.Exists(Path.Combine(dir.FullName, Marker))) return dir.FullName;
        }

        return null;
    }

    public static string BaselinesDir(string root) => Path.Combine(root, "bench", "baselines");
    public static string ResultsDir(string root) => Path.Combine(root, "bench-results");
    public static string PerformanceDocPath(string root) => Path.Combine(root, "docs", "performance.md");
    public static string RealConfigPath(string root) => Path.Combine(root, "bench", "real.local.json");

    public static string BaselinePath(string root, string machineKey, bool real) =>
        Path.Combine(BaselinesDir(root), machineKey + (real ? ".real" : "") + ".json");
}
