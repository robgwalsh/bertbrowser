using System.Diagnostics;
using System.Globalization;
using BertBrowser.Bench.Corpus;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>
/// Tier C: launches the built <c>BertBrowser.exe</c> several times against a scratch data directory
/// and reads back the marks the app writes under <c>BERTBROWSER_STARTUP_TRACE</c>.
/// </summary>
/// <remarks>
/// <para>
/// The one place anything in this repository starts the real executable, and sanctioned because the
/// user asked for this tier knowing a window appears. The app parks it offscreen and never activates
/// it, so nothing on the user's screen moves; it still exists for about a second per launch.
/// </para>
/// <para>
/// It refuses while a real BertBrowser is running: the single-instance mutex is per user, so the
/// launch would be a "second" copy and would measure a different path. <c>--allow-running</c> says
/// you know. The first launch creates and migrates the scratch database and is discarded as cold;
/// the rest reuse it, as every launch after the first on a real machine does.
/// </para>
/// </remarks>
internal static class StartupCommand
{
    private const int DefaultLaunches = 10;
    private static readonly TimeSpan LaunchTimeout = TimeSpan.FromSeconds(90);

    public static int Execute(ArgList args)
    {
        var launches = DefaultLaunches;
        string? app = null;
        string? outPath = null;
        string? repo = null;
        string? machineKey = null;
        var allowRunning = false;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--launches": launches = Math.Max(2, args.Int(arg)); break;
                case "--app": app = args.Value(arg); break;
                case "--out": outPath = args.Value(arg); break;
                case "--repo": repo = args.Value(arg); break;
                case "--machine-key": machineKey = args.Value(arg); break;
                case "--allow-running": allowRunning = true; break;
                default: throw ArgList.Unknown("startup", arg);
            }
        }

        var root = RepoRoot.Find(repo);
        app ??= Path.Combine(root, "src", "BertBrowser.App", "bin", "Release", "net10.0-windows", "BertBrowser.exe");
        if (!File.Exists(app))
        {
            Console.Error.WriteLine($"No app at {app}. Build with `dotnet build bertbrowser.sln -c Release`, or pass --app.");
            return Exit.Errors;
        }

        if (EnvironmentProbe.IsRunning("BertBrowser") && !allowRunning)
        {
            Console.Error.WriteLine("A BertBrowser is running. The single-instance claim is per user, so a measured launch " +
                                    "would start as a second copy and take a different path. Close it, or pass --allow-running " +
                                    "to measure that path on purpose.");
            return Exit.Errors;
        }

        if (EnvironmentProbe.IsRunning("BertBrowser.Indexer"))
            Console.WriteLine("# note: the index helper is running. A traced launch does not attach to it, but it is tailing the journal.");

        var corpusRoot = BenchCorpus.DefaultRoot;
        var tree = BenchCorpus.EnsureTree(corpusRoot, 1, Console.WriteLine);

        var work = Path.Combine(corpusRoot, "startup");
        var state = Path.Combine(work, "state");
        var traces = Path.Combine(work, "traces");
        if (Directory.Exists(work)) Directory.Delete(work, recursive: true);
        Directory.CreateDirectory(state);
        Directory.CreateDirectory(traces);

        Console.WriteLine($"# {launches} launches of {app}, first discarded; data dir {state}");

        var kept = new List<(StartupTraceData Trace, double LifetimeMs)>();
        string? failure = null;

        for (var k = 0; k < launches && failure is null; k++)
        {
            var tracePath = Path.Combine(traces, $"trace-{k:00}.json");
            var result = Launch(app, tree.Startup, state, tracePath);

            if (result.Error is not null)
            {
                failure = $"launch {k}: {result.Error}";
                break;
            }

            var trace = result.Trace!;
            var first = trace.MarksMs.TryGetValue(StartupMarks.FirstListing, out var fl) ? $"{fl:0} ms" : "no first listing";
            Console.WriteLine($"#   launch {k}{(k == 0 ? " (cold, discarded)" : "")}: first listing at {first}, " +
                              $"exit after {result.LifetimeMs:0} ms, instance {trace.Instance}" +
                              (trace.Error is null ? "" : $", error: {trace.Error}"));

            if (trace.Error is not null)
            {
                failure = $"launch {k}: {trace.Error}";
                break;
            }

            if (k > 0) kept.Add((trace, result.LifetimeMs));
        }

        var parameters = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["launches"] = launches.ToString(CultureInfo.InvariantCulture),
            ["discarded"] = "1",
            ["startFolder"] = "Startup(500)",
            ["parked"] = "true",
            ["app"] = Path.GetRelativePath(root, app),
        };

        var entries = new List<BenchEntry>();
        if (failure is not null)
        {
            Console.Error.WriteLine($"# {failure}; traces under {traces}");
            entries.Add(new BenchEntry("startup.firstListing", "C", parameters, 0, null, null, null, failure));
        }
        else
        {
            parameters["instance"] = kept.GroupBy(t => t.Trace.Instance).OrderByDescending(g => g.Count()).First().Key;

            var markNames = kept.SelectMany(t => t.Trace.MarksMs.Keys).Distinct(StringComparer.Ordinal);
            foreach (var mark in markNames)
            {
                var samples = kept.Where(t => t.Trace.MarksMs.ContainsKey(mark)).Select(t => t.Trace.MarksMs[mark]).ToList();
                entries.Add(new BenchEntry($"startup.{mark}", "C", parameters, samples.Count, TimeStats.From(samples, "ms"), null, null, null));
            }

            entries.Add(new BenchEntry("startup.processLifetime", "C", parameters, kept.Count,
                TimeStats.From(kept.Select(t => t.LifetimeMs).ToList(), "ms"), null, null, null));
        }

        var set = new BenchResultSet(
            BenchResultSet.CurrentSchemaVersion,
            DateTime.UtcNow,
            $"{BenchCli.ToolName} {RunCommand.ToolVersion()}",
            EnvironmentProbe.Git(root),
            EnvironmentProbe.Machine(machineKey),
            new RunInfo("startup", 1, false, ["C"], null, launches),
            entries);

        outPath ??= Path.Combine(RepoRoot.ResultsDir(root), "startup.json");
        BenchResultsJson.Write(set, outPath);

        Console.WriteLine();
        Console.WriteLine(BenchMarkdown.RenderResults(set));
        Console.WriteLine($"# → {outPath}");

        if (failure is null)
        {
            try { Directory.Delete(work, recursive: true); }
            catch (IOException) { }
        }

        return failure is null ? Exit.Ok : Exit.Errors;
    }

    private sealed record LaunchResult(StartupTraceData? Trace, double LifetimeMs, string? Error);

    private static LaunchResult Launch(string app, string startFolder, string dataDir, string tracePath)
    {
        var info = new ProcessStartInfo(app)
        {
            UseShellExecute = false,
            WorkingDirectory = Path.GetDirectoryName(app)!,
        };
        info.ArgumentList.Add(startFolder);
        info.Environment["BERTBROWSER_DATA_DIR"] = dataDir;
        info.Environment["BERTBROWSER_STARTUP_TRACE"] = tracePath;
        info.Environment["BERTBROWSER_EXIT_AFTER_STARTUP"] = "1";

        var clock = Stopwatch.StartNew();
        using var process = Process.Start(info);
        if (process is null) return new LaunchResult(null, 0, "the process did not start");

        if (!process.WaitForExit((int)LaunchTimeout.TotalMilliseconds))
        {
            process.Kill(entireProcessTree: true);
            return new LaunchResult(null, clock.Elapsed.TotalMilliseconds, $"still running after {LaunchTimeout.TotalSeconds:0} s; killed");
        }

        var lifetime = clock.Elapsed.TotalMilliseconds;
        if (!File.Exists(tracePath))
            return new LaunchResult(null, lifetime, $"exited {process.ExitCode} without writing a trace");

        try
        {
            return new LaunchResult(StartupTraceJson.Read(tracePath), lifetime, null);
        }
        catch (Exception e) when (e is IOException or InvalidDataException or System.Text.Json.JsonException)
        {
            return new LaunchResult(null, lifetime, $"unreadable trace: {e.Message}");
        }
    }
}
