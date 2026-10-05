using System.Diagnostics;
using System.Reflection;
using BenchmarkDotNet.Running;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>Tier A: runs the BenchmarkDotNet suite and writes one results file.</summary>
internal static class RunCommand
{
    public static int Execute(ArgList args)
    {
        var job = "default";
        var toolchain = "default";
        var filters = new List<string>();
        var scale = 1;
        var real = false;
        string? outPath = null;
        string? machineKey = null;
        string? repo = null;
        var ci = false;
        var allowDebug = false;
        var refreshReal = false;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--job": job = args.Value(arg); break;
                case "--toolchain": toolchain = args.Value(arg); break;
                case "--filter": filters.Add(args.Value(arg)); break;
                case "--scale": scale = Math.Max(1, args.Int(arg)); break;
                case "--real": real = true; break;
                case "--refresh-real": real = true; refreshReal = true; break;
                case "--out": outPath = args.Value(arg); break;
                case "--machine-key": machineKey = args.Value(arg); break;
                case "--repo": repo = args.Value(arg); break;
                case "--ci": ci = true; break;
                case "--allow-debug": allowDebug = true; break;
                default: throw ArgList.Unknown("run", arg);
            }
        }

        if (ci) job = "short";

        var root = RepoRoot.Find(repo);

        if (IsDebugBuild() && !allowDebug)
        {
            Console.Error.WriteLine("This is a Debug build. Benchmark numbers from unoptimised code are not worth recording; " +
                                    "build with -c Release, or pass --allow-debug to run anyway.");
            return Exit.Errors;
        }

        var corpusRoot = BenchCorpus.DefaultRoot;
        string? resolvedReal = null;
        if (real)
        {
            var configPath = RepoRoot.RealConfigPath(root);
            if (!File.Exists(configPath))
            {
                Console.Error.WriteLine($"--real needs {configPath}; see bench/README.md for its shape.");
                return Exit.Errors;
            }

            // The backup is taken here, once, in the host: the benchmark processes only ever read it.
            var realConfig = RealCorpus.Load(configPath);
            var copy = RealCorpus.EnsureBackup(realConfig, corpusRoot, Console.WriteLine, refreshReal);
            resolvedReal = RealCorpus.WriteResolved(realConfig with { DbCopyPath = copy }, corpusRoot);
        }

        var context = new BenchContext(scale, corpusRoot, resolvedReal);
        BenchCorpus.EnsureAll(context.CorpusRoot, scale, Console.WriteLine);
        context.Publish();

        var config = BenchConfig.Create(job, toolchain, BenchCorpus.ArtifactsPath(context.CorpusRoot));

        var bdnArgs = filters.Count == 0
            ? new[] { "--filter", "*" }
            // One --filter with every glob after it: BenchmarkDotNet refuses the option twice.
            : ["--filter", .. filters];

        var clock = Stopwatch.StartNew();
        var summaries = BenchmarkSwitcher.FromAssembly(typeof(RunCommand).Assembly).Run(bdnArgs, config).ToList();
        var entries = SummaryAdapter.ToEntries(summaries, scale);

        if (entries.Count == 0)
        {
            Console.Error.WriteLine("No benchmarks matched. Filters are BenchmarkDotNet globs over Namespace.Class.Method, " +
                                    "for example --filter *PathKey*.");
            return Exit.Errors;
        }

        var set = new BenchResultSet(
            BenchResultSet.CurrentSchemaVersion,
            DateTime.UtcNow,
            $"{BenchCli.ToolName} {ToolVersion()}",
            EnvironmentProbe.Git(root),
            EnvironmentProbe.Machine(machineKey),
            new RunInfo(job, scale, real, ["A"], null, null, toolchain),
            entries);

        outPath ??= Path.Combine(RepoRoot.ResultsDir(root), real ? "current.real.json" : "current.json");
        BenchResultsJson.Write(set, outPath);

        Console.WriteLine();
        Console.WriteLine(BenchMarkdown.RenderResults(set));
        Console.WriteLine($"# {entries.Count} benchmarks in {clock.Elapsed.TotalMinutes:0.0} min → {outPath}");

        var failed = entries.Count(e => e.Error is not null);
        if (failed > 0)
        {
            Console.Error.WriteLine($"# {failed} benchmark(s) failed; see the entries marked error.");
            return Exit.Errors;
        }

        return Exit.Ok;
    }

    private static bool IsDebugBuild() =>
        typeof(RunCommand).Assembly.GetCustomAttribute<DebuggableAttribute>() is { IsJITOptimizerDisabled: true };

    internal static string ToolVersion() =>
        typeof(RunCommand).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";
}
