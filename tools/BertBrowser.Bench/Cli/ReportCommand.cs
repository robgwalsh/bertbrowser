using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>Regenerates <c>docs/performance.md</c> from every committed baseline.</summary>
internal static class ReportCommand
{
    /// <summary>The "how to run" block the document opens with. Kept here, beside the CLI it describes.</summary>
    private const string Commands = """
        ```powershell
        dotnet build bertbrowser.sln -c Release
        $bench = "tools\BertBrowser.Bench\bin\Release\net10.0\BertBrowser.Bench.exe"

        & $bench run                                    # Tier A, default job (long; --filter *PathKey* to narrow)
        & $bench run --job short                        # Tier A, quick pass
        & $bench ui --repeat 5                          # Tier B, each tools/ui/bench-*.bbs five times
        & $bench startup --launches 10                  # Tier C, the real exe (a window is shown offscreen)

        & $bench compare --baseline bench\baselines\<key>.json --current bench-results\current.json
        & $bench baseline --from bench-results\current.json --from bench-results\ui.json --from bench-results\startup.json
        & $bench report                                 # this file
        ```
        """;

    public static int Execute(ArgList args)
    {
        var check = false;
        string? repo = null;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--check": check = true; break;
                case "--repo": repo = args.Value(arg); break;
                default: throw ArgList.Unknown("report", arg);
            }
        }

        var root = RepoRoot.Find(repo);
        var dir = RepoRoot.BaselinesDir(root);
        var baselines = Directory.Exists(dir)
            ? Directory.GetFiles(dir, "*.json").Order(StringComparer.Ordinal).Select(BenchResultsJson.Read).ToList()
            : [];

        var markdown = PerformanceDoc.Render(baselines, Commands);
        var path = RepoRoot.PerformanceDocPath(root);

        if (check)
        {
            var existing = File.Exists(path) ? File.ReadAllText(path) : "";
            if (Normalise(existing) == Normalise(markdown))
            {
                Console.WriteLine($"{path} is up to date.");
                return Exit.Ok;
            }

            Console.Error.WriteLine($"{path} is out of date; run `{BenchCli.ToolName} report` and commit it.");
            return Exit.Regression;
        }

        File.WriteAllText(path, markdown);
        Console.WriteLine($"Wrote {path} from {baselines.Count} baseline file(s).");
        return Exit.Ok;
    }

    private static string Normalise(string text) => text.Replace("\r\n", "\n").TrimEnd();
}
