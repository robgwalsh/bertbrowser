using System.Globalization;
using System.Text;

namespace BertBrowser.Core.Benchmarking;

/// <summary>Renders results and comparisons as GitHub-flavoured Markdown — the console, the CI job
/// summary and <c>docs/performance.md</c> all read this.</summary>
public static class BenchMarkdown
{
    /// <summary>A comparison, regressions first.</summary>
    public static string RenderCompare(CompareReport report, CompareOptions options, string title)
    {
        var sb = new StringBuilder();
        sb.Append("## ").AppendLine(title).AppendLine();

        var counts = report.Rows
            .GroupBy(r => r.Status)
            .OrderBy(g => g.Key)
            .Select(g => $"{g.Count()} {Describe(g.Key)}");
        sb.Append(report.Rows.Count).Append(" benchmarks: ").AppendLine(string.Join(", ", counts)).AppendLine();

        sb.Append("Time gate ").Append(options.GateTime ? "on" : "off (reported only)")
          .Append(" at ").Append(Pct(options.TimeThresholdPct)).Append("; allocation gate ")
          .Append(options.GateAlloc ? "on" : "off").Append(options.AllocTolerancePctOverride is { } tol
              ? $" at {Pct(tol)} for every benchmark."
              : " at each benchmark's own tolerance.")
          .Append(" Process-memory gate ").Append(options.GateMemory ? "on" : "off (reported only)")
          .Append(": managed heap at ").Append(Pct(options.MemoryThresholdPct)).Append(" over ")
          .Append(Bytes(options.MemoryNoiseFloorBytes)).Append(", private bytes at ")
          .Append(Pct(options.PrivateBytesThresholdPct)).Append(" over ")
          .Append(Bytes(options.PrivateBytesNoiseFloorBytes)).Append('.')
          .AppendLine().AppendLine();

        // A snapshot has no time and no per-operation allocation, so in the table below it would be
        // a row of dashes; it gets its own, with the three numbers it does have.
        var ordered = report.Rows.OrderBy(r => r.Status).ThenBy(r => r.Id, StringComparer.Ordinal).ToList();
        var snapshots = ordered.Where(r => r.BaselineProcess is not null || r.CurrentProcess is not null).ToList();

        sb.AppendLine("| Benchmark | Tier | Base | Current | Δ | Alloc base | Alloc now | Δ | Status |");
        sb.AppendLine("|---|---|---:|---:|---:|---:|---:|---:|---|");

        foreach (var row in ordered.Except(snapshots))
        {
            sb.Append("| `").Append(row.Id).Append("` | ").Append(row.Tier)
              .Append(" | ").Append(Time(row.BaselineMedian, row.Unit))
              .Append(" | ").Append(Time(row.CurrentMedian, row.Unit))
              .Append(" | ").Append(Delta(row.TimeDeltaPct))
              .Append(" | ").Append(Bytes(row.BaselineAlloc))
              .Append(" | ").Append(Bytes(row.CurrentAlloc))
              .Append(" | ").Append(Delta(row.AllocDeltaPct))
              .Append(" | ").Append(Describe(row.Status));
            if (row.Note is not null) sb.Append(" — ").Append(row.Note.Replace('|', '/'));
            sb.AppendLine(" |");
        }

        if (snapshots.Count > 0)
        {
            sb.AppendLine();
            sb.AppendLine("### Process memory");
            sb.AppendLine();
            sb.AppendLine("| Snapshot | Tier | Private base | Private now | Δ | Managed base | Managed now | Δ | Working set base | Working set now | Status |");
            sb.AppendLine("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|");

            foreach (var row in snapshots)
            {
                var was = row.BaselineProcess;
                var now = row.CurrentProcess;
                sb.Append("| `").Append(row.Id).Append("` | ").Append(row.Tier)
                  .Append(" | ").Append(Bytes(was?.PrivateBytes))
                  .Append(" | ").Append(Bytes(now?.PrivateBytes))
                  .Append(" | ").Append(Delta(row.MemoryDeltaPct))
                  .Append(" | ").Append(Bytes(was?.ManagedHeapBytes))
                  .Append(" | ").Append(Bytes(now?.ManagedHeapBytes))
                  .Append(" | ").Append(Delta(DeltaPct(was?.ManagedHeapBytes, now?.ManagedHeapBytes)))
                  .Append(" | ").Append(Bytes(was?.WorkingSetBytes))
                  .Append(" | ").Append(Bytes(now?.WorkingSetBytes))
                  .Append(" | ").Append(Describe(row.Status));
                if (row.Note is not null) sb.Append(" — ").Append(row.Note.Replace('|', '/'));
                sb.AppendLine(" |");
            }
        }

        return sb.ToString();
    }

    /// <summary>A results file on its own, one table per tier.</summary>
    public static string RenderResults(BenchResultSet set)
    {
        var sb = new StringBuilder();
        sb.Append("## Results — ").Append(set.Machine.Key).Append(" — ")
          .AppendLine(set.GeneratedUtc.ToString("yyyy-MM-dd HH:mm 'UTC'", CultureInfo.InvariantCulture))
          .AppendLine();
        AppendTierTables(sb, set.Benchmarks);
        return sb.ToString();
    }

    /// <summary>The per-tier tables, shared with <see cref="PerformanceDoc"/>.</summary>
    internal static void AppendTierTables(StringBuilder sb, IReadOnlyList<BenchEntry> entries)
    {
        foreach (var tier in entries.GroupBy(e => e.Tier).OrderBy(g => g.Key, StringComparer.Ordinal))
        {
            sb.Append("### Tier ").Append(tier.Key).Append(" — ").AppendLine(TierName(tier.Key)).AppendLine();

            switch (tier.Key)
            {
                case "A":
                    sb.AppendLine("| Benchmark | Median | p95 | Allocated/op | Gen0/kop | n |");
                    sb.AppendLine("|---|---:|---:|---:|---:|---:|");
                    foreach (var e in tier.OrderBy(e => e.Id, StringComparer.Ordinal))
                    {
                        sb.Append("| `").Append(e.Id).Append("` | ");
                        if (e.Error is not null) { sb.Append("error: ").Append(e.Error.Replace('|', '/')).AppendLine(" | | | | |"); continue; }
                        sb.Append(Time(e.Time?.Median, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.P95, e.Time?.Unit)).Append(" | ")
                          .Append(Bytes(e.Memory?.AllocatedBytes)).Append(" | ")
                          .Append(e.Memory is null ? "—" : e.Memory.Gen0PerKOps.ToString("0.##", CultureInfo.InvariantCulture)).Append(" | ")
                          .Append(e.N).AppendLine(" |");
                    }
                    break;

                case "B":
                    sb.AppendLine("| Scenario | Median | p95 | n |");
                    sb.AppendLine("|---|---:|---:|---:|");
                    foreach (var e in tier.Where(e => e.Process is null).OrderBy(e => e.Id, StringComparer.Ordinal))
                    {
                        sb.Append("| `").Append(e.Id).Append("` | ");
                        if (e.Error is not null) { sb.Append("error: ").Append(e.Error.Replace('|', '/')).AppendLine(" | | |"); continue; }
                        sb.Append(Time(e.Time?.Median, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.P95, e.Time?.Unit)).Append(" | ")
                          .Append(e.N).AppendLine(" |");
                    }
                    AppendMemoryTable(sb, tier, rows: true);
                    break;

                default:
                    sb.AppendLine("| Mark | Median | p95 | Min | n |");
                    sb.AppendLine("|---|---:|---:|---:|---:|");
                    foreach (var e in tier.Where(e => e.Process is null).OrderBy(e => e.Time?.Median ?? double.MaxValue).ThenBy(e => e.Id, StringComparer.Ordinal))
                    {
                        sb.Append("| `").Append(e.Id).Append("` | ");
                        if (e.Error is not null) { sb.Append("error: ").Append(e.Error.Replace('|', '/')).AppendLine(" | | | |"); continue; }
                        sb.Append(Time(e.Time?.Median, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.P95, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.Min, e.Time?.Unit)).Append(" | ")
                          .Append(e.N).AppendLine(" |");
                    }
                    AppendMemoryTable(sb, tier, rows: false);
                    break;
            }

            sb.AppendLine();
        }
    }

    /// <summary>
    /// The tier's process snapshots, if it has any, as a table of their own under its timings.
    /// </summary>
    /// <param name="rows">Whether the snapshots know how many rows the list held (Tier B does; the
    /// real executable in Tier C has nobody to ask).</param>
    private static void AppendMemoryTable(StringBuilder sb, IEnumerable<BenchEntry> tier, bool rows)
    {
        var snapshots = tier.Where(e => e.Process is not null).OrderBy(e => e.Id, StringComparer.Ordinal).ToList();
        if (snapshots.Count == 0) return;

        sb.AppendLine();
        sb.Append("| Memory snapshot | Managed heap | Private bytes | Working set | Peak working set |")
          .AppendLine(rows ? " Items | Realized | n |" : " n |");
        sb.Append("|---|---:|---:|---:|---:|").AppendLine(rows ? "---:|---:|---:|" : "---:|");

        foreach (var e in snapshots)
        {
            var p = e.Process!;
            sb.Append("| `").Append(e.Id).Append("` | ")
              .Append(Bytes(p.ManagedHeapBytes)).Append(" | ")
              .Append(Bytes(p.PrivateBytes)).Append(" | ")
              .Append(Bytes(p.WorkingSetBytes)).Append(" | ")
              .Append(Bytes(p.PeakWorkingSetBytes)).Append(" | ");
            if (rows)
            {
                sb.Append(p.Items.ToString(CultureInfo.InvariantCulture)).Append(" | ")
                  .Append(p.RealizedRows.ToString(CultureInfo.InvariantCulture)).Append(" | ");
            }
            sb.Append(e.N).AppendLine(" |");
        }
    }

    internal static string TierName(string tier) => tier switch
    {
        "A" => "Core micro-benchmarks (BenchmarkDotNet)",
        "B" => "UI scenarios (offscreen harness)",
        "C" => "Startup of the real executable",
        _ => tier,
    };

    private static string Describe(CompareStatus status) => status switch
    {
        CompareStatus.AllocUp => "allocates more",
        CompareStatus.MemoryUp => "holds more memory",
        CompareStatus.Slower => "slower",
        CompareStatus.Errored => "errored",
        CompareStatus.Removed => "removed",
        CompareStatus.New => "new",
        CompareStatus.AllocDown => "allocates less",
        CompareStatus.MemoryDown => "holds less memory",
        CompareStatus.Faster => "faster",
        _ => "same",
    };

    /// <summary>A duration in its unit, scaled to read well: <c>12.3 µs</c>, <c>812 ms</c>, <c>1.21 s</c>.</summary>
    internal static string Time(double? value, string? unit)
    {
        if (value is not { } v || unit is null) return "—";

        var ns = unit switch
        {
            "ns" => v,
            "ms" => v * 1_000_000,
            "s" => v * 1_000_000_000,
            _ => double.NaN,
        };
        if (double.IsNaN(ns)) return $"{Num(v)} {unit}";

        return ns switch
        {
            < 1_000 => $"{Num(ns)} ns",
            < 1_000_000 => $"{Num(ns / 1_000)} µs",
            < 1_000_000_000 => $"{Num(ns / 1_000_000)} ms",
            _ => $"{Num(ns / 1_000_000_000)} s",
        };
    }

    internal static string Bytes(long? value)
    {
        if (value is not { } b) return "—";
        return b switch
        {
            < 1024 => $"{b} B",
            < 1024 * 1024 => $"{Num(b / 1024.0)} KB",
            < 1024L * 1024 * 1024 => $"{Num(b / (1024.0 * 1024))} MB",
            _ => $"{Num(b / (1024.0 * 1024 * 1024))} GB",
        };
    }

    private static string Delta(double? pct) => pct is { } p
        ? (p >= 0 ? "+" : "") + p.ToString("0.0", CultureInfo.InvariantCulture) + "%"
        : "—";

    private static double? DeltaPct(long? was, long? now) =>
        was is > 0 && now is { } n ? (n - was.Value) / (double)was.Value * 100 : null;

    private static string Pct(double pct) => pct.ToString("0.#", CultureInfo.InvariantCulture) + "%";

    /// <summary>Three significant figures, no exponent.</summary>
    private static string Num(double v)
    {
        var a = Math.Abs(v);
        var format = a >= 100 ? "0" : a >= 10 ? "0.0" : "0.00";
        return v.ToString(format, CultureInfo.InvariantCulture);
    }
}
