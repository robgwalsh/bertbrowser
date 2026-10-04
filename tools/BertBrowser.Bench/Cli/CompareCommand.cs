using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>Judges a results file against a baseline and says so in Markdown.</summary>
internal static class CompareCommand
{
    public static int Execute(ArgList args)
    {
        string? baselinePath = null;
        string? currentPath = null;
        string? summaryPath = null;
        string? title = null;
        var options = new CompareOptions();
        var ci = false;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--baseline": baselinePath = args.Value(arg); break;
                case "--current": currentPath = args.Value(arg); break;
                case "--time-threshold": options = options with { TimeThresholdPct = args.Double(arg) }; break;
                case "--alloc-threshold": options = options with { AllocTolerancePctOverride = args.Double(arg) }; break;
                case "--no-time": options = options with { GateTime = false }; break;
                case "--no-alloc": options = options with { GateAlloc = false }; break;
                case "--strict": options = options with { Strict = true }; break;
                case "--summary": summaryPath = args.Value(arg); break;
                case "--title": title = args.Value(arg); break;
                case "--ci": ci = true; break;
                case "--repo": args.Value(arg); break;
                default: throw ArgList.Unknown("compare", arg);
            }
        }

        if (baselinePath is null || currentPath is null)
            throw new UsageException("compare needs --baseline <file> and --current <file>.");

        if (ci) options = options with { GateTime = false };

        var current = BenchResultsJson.Read(currentPath);
        title ??= $"Benchmarks — {current.Machine.Key} — {current.Git.Sha[..Math.Min(9, current.Git.Sha.Length)]}";

        if (!File.Exists(baselinePath))
        {
            if (!ci)
            {
                Console.Error.WriteLine($"No baseline at {baselinePath}. Record one with: " +
                                        $"{BenchCli.ToolName} baseline --from {currentPath}");
                return Exit.Errors;
            }

            // CI bootstraps itself: the first run has nothing to compare against, so it hands back a
            // candidate for a person to commit and does not fail.
            var candidate = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(currentPath))!, "candidate-baseline.json");
            BenchResultsJson.Write(current, candidate);
            var notice = $"No baseline at `{baselinePath}`. This run's results were written to `{candidate}`; " +
                         $"download the artifact and commit that file as the baseline to turn the allocation gate on.";
            Console.WriteLine(notice);
            WriteSummary(summaryPath, ci, $"## {title}\n\n{notice}\n\n{BenchMarkdown.RenderResults(current)}");
            return current.Benchmarks.Any(b => b.Error is not null) ? Exit.Errors : Exit.Ok;
        }

        var baseline = BenchResultsJson.Read(baselinePath);
        var report = BenchCompare.Compare(baseline, current, options);
        var markdown = BenchMarkdown.RenderCompare(report, options, title);

        // Measured: the in-process and default toolchains disagree about what SQLite and the hashing
        // APIs allocate (MD5 over 64 MB reads 333 B one way and 1.6 KB the other), and the short job
        // takes too few samples for its times to mean much. A mismatch is not refused — someone may
        // want the picture anyway — but it is said out loud, above the table.
        var mismatch = new List<string>();
        if (!string.Equals(baseline.Run.Job, current.Run.Job, StringComparison.Ordinal))
            mismatch.Add($"job {baseline.Run.Job} against {current.Run.Job}");
        if (baseline.Run.Toolchain is not null && current.Run.Toolchain is not null &&
            !string.Equals(baseline.Run.Toolchain, current.Run.Toolchain, StringComparison.Ordinal))
            mismatch.Add($"toolchain {baseline.Run.Toolchain} against {current.Run.Toolchain}");
        if (mismatch.Count > 0)
        {
            markdown = $"> **Not like for like:** {string.Join("; ", mismatch)}. Allocation and time differences " +
                       "between jobs or toolchains are expected and are not regressions. Record a baseline with " +
                       $"the same settings to compare properly.\n\n{markdown}";
        }

        Console.WriteLine(markdown);
        WriteSummary(summaryPath, ci, markdown);

        if (report.HasErrors)
        {
            Console.Error.WriteLine("# benchmarks errored — see the rows marked errored.");
            return Exit.Errors;
        }

        var regressed = report.TimeRegressed || report.AllocRegressed || (options.Strict && report.HasRemoved);
        if (regressed)
        {
            Console.Error.WriteLine("# regression: " + string.Join(", ", Reasons(report, options)));
            return Exit.Regression;
        }

        Console.WriteLine("# no regressions.");
        return Exit.Ok;
    }

    private static IEnumerable<string> Reasons(CompareReport report, CompareOptions options)
    {
        if (report.AllocRegressed) yield return "more allocation than the baseline allows";
        if (report.TimeRegressed) yield return $"slower than the baseline by more than {options.TimeThresholdPct:0.#}%";
        if (options.Strict && report.HasRemoved) yield return "benchmarks missing from this run (--strict)";
    }

    private static void WriteSummary(string? summaryPath, bool ci, string markdown)
    {
        if (summaryPath is not null)
        {
            File.WriteAllText(summaryPath, markdown);
        }

        if (ci && Environment.GetEnvironmentVariable("GITHUB_STEP_SUMMARY") is { Length: > 0 } stepSummary)
        {
            File.AppendAllText(stepSummary, markdown + Environment.NewLine);
        }
    }
}
