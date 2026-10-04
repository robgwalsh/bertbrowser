using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>
/// Tier B: runs each <c>tools/ui/bench-*.bbs</c> through the offscreen harness several times, each
/// in a fresh process, and aggregates the <c>time</c> and <c>mem</c> samples it emits.
/// </summary>
/// <remarks>
/// <para>
/// A fresh process per run is deliberate: every run is a cold app with an empty scratch data
/// directory, so the K samples of one scenario are K independent measurements rather than K laps of
/// one warming cache. The harness's own startup is one of the samples (<c>startup.hosted</c>).
/// </para>
/// <para>
/// A scenario's parameters travel in its script, on a <c>#!</c> line (<c>#! files=10000 dirs=50</c>),
/// so the result carries its denominator without this tool knowing anything about the fixture.
/// </para>
/// </remarks>
internal static class UiCommand
{
    private const string ScriptPrefix = "bench-";
    private const int DefaultRepeat = 5;
    private const int DefaultTimeoutSeconds = 600;
    private const int DefaultBusyTimeoutMs = 120_000;

    public static int Execute(ArgList args)
    {
        var scenarios = new List<string>();
        var repeat = DefaultRepeat;
        string? harness = null;
        string? outPath = null;
        string? repo = null;
        string? machineKey = null;
        var timeout = DefaultTimeoutSeconds;
        var busyTimeout = DefaultBusyTimeoutMs;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--scenario": scenarios.Add(args.Value(arg)); break;
                case "--repeat": repeat = Math.Max(1, args.Int(arg)); break;
                case "--harness": harness = args.Value(arg); break;
                case "--out": outPath = args.Value(arg); break;
                case "--repo": repo = args.Value(arg); break;
                case "--machine-key": machineKey = args.Value(arg); break;
                case "--timeout": timeout = args.Int(arg); break;
                case "--busy-timeout": busyTimeout = args.Int(arg); break;
                default: throw ArgList.Unknown("ui", arg);
            }
        }

        var root = RepoRoot.Find(repo);
        harness ??= Path.Combine(root, "tools", "BertBrowser.Harness", "bin", "Release", "net10.0-windows", "BertBrowser.Harness.exe");
        if (!File.Exists(harness))
        {
            Console.Error.WriteLine($"No harness at {harness}. Build with `dotnet build bertbrowser.sln -c Release`, or pass --harness.");
            return Exit.Errors;
        }

        var scriptDir = Path.Combine(root, "tools", "ui");
        var scripts = scenarios.Count == 0
            ? Directory.GetFiles(scriptDir, ScriptPrefix + "*.bbs").Order(StringComparer.Ordinal).ToList()
            : scenarios.Select(s => Path.Combine(scriptDir, ScriptPrefix + s.TrimStart('-') + ".bbs")).ToList();

        foreach (var missing in scripts.Where(s => !File.Exists(s)))
            throw new UsageException($"No scenario script at {missing}.");

        if (scripts.Count == 0)
        {
            Console.Error.WriteLine($"No {ScriptPrefix}*.bbs scripts under {scriptDir}.");
            return Exit.Errors;
        }

        var work = Path.Combine(Path.GetTempPath(), "bertbrowser-bench", "ui", DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture));
        Directory.CreateDirectory(work);

        var entries = new List<BenchEntry>();
        var clock = Stopwatch.StartNew();
        var anyFailed = false;

        foreach (var script in scripts)
        {
            var scenario = Path.GetFileNameWithoutExtension(script)[ScriptPrefix.Length..];
            var parameters = ReadParameters(script);
            parameters["renderMode"] = "SoftwareOnly";
            parameters["repeat"] = repeat.ToString(CultureInfo.InvariantCulture);

            Console.WriteLine($"# {scenario}: {repeat} run(s)");
            var samples = new List<IReadOnlyList<Sample>>();
            string? failure = null;

            for (var k = 1; k <= repeat && failure is null; k++)
            {
                var runDir = Path.Combine(work, scenario, k.ToString(CultureInfo.InvariantCulture));
                var benchOut = Path.Combine(runDir, "samples.jsonl");
                var log = Path.Combine(runDir, "harness.log");
                Directory.CreateDirectory(runDir);

                var exit = RunHarness(harness, script, runDir, benchOut, log, timeout, busyTimeout);
                if (exit != 0 || !File.Exists(benchOut))
                {
                    failure = $"harness exit {exit} on run {k}; see {log}";
                    break;
                }

                samples.Add(ReadSamples(benchOut));
                Console.WriteLine($"#   run {k}: {string.Join(", ", samples[^1].Where(s => s.Ms is not null).Select(s => $"{s.Name} {s.Ms:0} ms"))}");
            }

            if (failure is not null)
            {
                anyFailed = true;
                Console.Error.WriteLine($"# {scenario}: {failure}");
                entries.Add(new BenchEntry($"ui.{scenario}", "B", parameters, 0, null, null, null, failure));
                continue;
            }

            entries.AddRange(Aggregate(scenario, parameters, samples));
        }

        var set = new BenchResultSet(
            BenchResultSet.CurrentSchemaVersion,
            DateTime.UtcNow,
            $"{BenchCli.ToolName} {RunCommand.ToolVersion()}",
            EnvironmentProbe.Git(root),
            EnvironmentProbe.Machine(machineKey),
            new RunInfo("ui", 1, false, ["B"], "SoftwareOnly", repeat),
            entries);

        outPath ??= Path.Combine(RepoRoot.ResultsDir(root), "ui.json");
        BenchResultsJson.Write(set, outPath);

        Console.WriteLine();
        Console.WriteLine(BenchMarkdown.RenderResults(set));
        Console.WriteLine($"# {entries.Count} entries from {scripts.Count} scenario(s) in {clock.Elapsed.TotalMinutes:0.0} min → {outPath}");

        if (anyFailed)
        {
            Console.Error.WriteLine($"# some scenarios failed; their logs are under {work}");
            return Exit.Errors;
        }

        try { Directory.Delete(work, recursive: true); }
        catch (IOException) { /* a straggler; %TEMP% is not precious */ }

        return Exit.Ok;
    }

    private static int RunHarness(string harness, string script, string outDir, string benchOut, string log, int timeout, int busyTimeout)
    {
        var info = new ProcessStartInfo(harness)
        {
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
        };
        info.ArgumentList.Add("--script"); info.ArgumentList.Add(script);
        info.ArgumentList.Add("--out"); info.ArgumentList.Add(outDir);
        info.ArgumentList.Add("--bench-out"); info.ArgumentList.Add(benchOut);
        info.ArgumentList.Add("--timeout"); info.ArgumentList.Add(timeout.ToString(CultureInfo.InvariantCulture));
        info.ArgumentList.Add("--busy-timeout"); info.ArgumentList.Add(busyTimeout.ToString(CultureInfo.InvariantCulture));

        using var process = Process.Start(info) ?? throw new InvalidOperationException($"Could not start {harness}.");
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        if (!process.WaitForExit((timeout + 30) * 1000))
        {
            process.Kill(entireProcessTree: true);
            File.WriteAllText(log, "killed: the harness outlived its own watchdog\n" + stdout.Result + stderr.Result);
            return -1;
        }

        File.WriteAllText(log, stdout.Result + stderr.Result);
        return process.ExitCode;
    }

    /// <summary><c>#! key=value key=value</c> on any line of the script.</summary>
    private static Dictionary<string, string> ReadParameters(string script)
    {
        var parameters = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var line in File.ReadLines(script))
        {
            if (!line.StartsWith("#!", StringComparison.Ordinal)) continue;
            foreach (var pair in line[2..].Split(' ', StringSplitOptions.RemoveEmptyEntries))
            {
                var eq = pair.IndexOf('=');
                if (eq > 0) parameters[pair[..eq]] = pair[(eq + 1)..];
            }
        }

        return parameters;
    }

    private static IReadOnlyList<Sample> ReadSamples(string path)
    {
        var samples = new List<Sample>();
        foreach (var line in File.ReadLines(path))
        {
            if (string.IsNullOrWhiteSpace(line)) continue;
            using var doc = JsonDocument.Parse(line);
            var o = doc.RootElement;
            var kind = o.GetProperty("kind").GetString() ?? "";
            var name = o.GetProperty("name").GetString() ?? "";

            samples.Add(kind == "time"
                ? new Sample(name, o.GetProperty("ms").GetDouble(), null)
                : new Sample(name, null, new ProcessStats(
                    o.GetProperty("workingSetBytes").GetInt64(),
                    o.GetProperty("privateBytes").GetInt64(),
                    o.GetProperty("managedBytes").GetInt64(),
                    o.GetProperty("realized").GetInt32(),
                    o.GetProperty("items").GetInt32(),
                    o.GetProperty("retainedThumbnails").GetInt32())));
        }

        return samples;
    }

    /// <summary>One entry per sample name: timings become <see cref="TimeStats"/>, memory readbacks
    /// become the median of each field.</summary>
    private static IEnumerable<BenchEntry> Aggregate(
        string scenario, IReadOnlyDictionary<string, string> parameters, IReadOnlyList<IReadOnlyList<Sample>> runs)
    {
        var names = runs.SelectMany(r => r.Select(s => s.Name)).Distinct(StringComparer.Ordinal).ToList();

        foreach (var name in names)
        {
            // Every run records its own startup; only the startup scenario is about it.
            if (name == "startup.hosted" && scenario != "startup") continue;

            var id = $"ui.{scenario}.{name}";
            var times = runs.SelectMany(r => r.Where(s => s.Name == name && s.Ms is not null)).Select(s => s.Ms!.Value).ToList();
            var mems = runs.SelectMany(r => r.Where(s => s.Name == name && s.Process is not null)).Select(s => s.Process!).ToList();

            if (times.Count > 0)
            {
                yield return new BenchEntry(id, "B", parameters, times.Count, TimeStats.From(times, "ms"), null, null, null);
            }
            else if (mems.Count > 0)
            {
                var process = new ProcessStats(
                    Median(mems.Select(m => (double)m.WorkingSetBytes)),
                    Median(mems.Select(m => (double)m.PrivateBytes)),
                    Median(mems.Select(m => (double)m.ManagedHeapBytes)),
                    (int)Median(mems.Select(m => (double)m.RealizedRows)),
                    (int)Median(mems.Select(m => (double)m.Items)),
                    (int)Median(mems.Select(m => (double)m.RetainedThumbnails)));
                yield return new BenchEntry(id, "B", parameters, mems.Count, null, null, process, null);
            }
        }
    }

    private static long Median(IEnumerable<double> values)
    {
        var sorted = values.Order().ToArray();
        if (sorted.Length == 0) return 0;
        var mid = sorted.Length / 2;
        return (long)(sorted.Length % 2 == 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2);
    }

    private sealed record Sample(string Name, double? Ms, ProcessStats? Process);
}
