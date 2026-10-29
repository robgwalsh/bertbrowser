using BertBrowser.Bench.Infrastructure;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.Bench.Cli;

/// <summary>
/// Records a baseline: one or more results files merged into
/// <c>bench/baselines/&lt;machine-key&gt;[.real].json</c>.
/// </summary>
/// <remarks>
/// Merging is what lets a Tier A run, a Tier B run and a Tier C run — three processes, three files —
/// land in one baseline per machine. Later files override earlier ones where ids coincide, so
/// re-recording one tier is <c>baseline --from &lt;the existing baseline&gt; --from &lt;the new file&gt;</c>.
/// </remarks>
internal static class BaselineCommand
{
    public static int Execute(ArgList args)
    {
        var from = new List<string>();
        string? machineKey = null;
        string? repo = null;

        while (args.TryNext(out var arg))
        {
            switch (arg)
            {
                case "--from": from.Add(args.Value(arg)); break;
                case "--machine-key": machineKey = args.Value(arg); break;
                case "--repo": repo = args.Value(arg); break;
                default: throw ArgList.Unknown("baseline", arg);
            }
        }

        if (from.Count == 0) throw new UsageException("baseline needs at least one --from <file>.");

        var root = RepoRoot.Find(repo);
        var sets = from.Select(BenchResultsJson.Read).ToList();

        var real = sets[0].Run.Real;
        if (sets.Any(s => s.Run.Real != real))
            throw new InvalidOperationException("Real and synthetic results cannot share a baseline; record them separately.");

        var merged = Merge(sets, machineKey);
        var path = RepoRoot.BaselinePath(root, merged.Machine.Key, real);
        var existed = File.Exists(path);
        BenchResultsJson.Write(merged, path);

        Console.WriteLine($"{(existed ? "Replaced" : "Recorded")} {path}: {merged.Benchmarks.Count} benchmarks, " +
                          $"tiers {string.Join("+", merged.Run.Tiers)}, at {merged.Git.Sha[..Math.Min(9, merged.Git.Sha.Length)]}" +
                          (merged.Git.Dirty ? " (tree had uncommitted changes)" : "") + ".");
        Console.WriteLine($"Run `{BenchCli.ToolName} report` to regenerate docs/performance.md, and commit both.");
        return Exit.Ok;
    }

    internal static BenchResultSet Merge(IReadOnlyList<BenchResultSet> sets, string? machineKey)
    {
        var entries = new Dictionary<string, BenchEntry>(StringComparer.Ordinal);
        var tiers = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var set in sets)
        {
            foreach (var entry in set.Benchmarks) entries[entry.Id] = entry;
            foreach (var tier in set.Run.Tiers) tiers.Add(tier);
        }

        var first = sets[0];
        var last = sets[^1];
        var machine = first.Machine with { Key = machineKey ?? first.Machine.Key };

        return new BenchResultSet(
            BenchResultSet.CurrentSchemaVersion,
            DateTime.UtcNow,
            $"{BenchCli.ToolName} {RunCommand.ToolVersion()}",
            last.Git,
            machine,
            first.Run with { Tiers = [.. tiers] },
            [.. entries.Values.OrderBy(e => e.Id, StringComparer.Ordinal)]);
    }
}
