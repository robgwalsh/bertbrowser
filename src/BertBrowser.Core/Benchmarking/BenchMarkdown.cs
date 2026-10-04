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
          .AppendLine().AppendLine();

        sb.AppendLine("| Benchmark | Tier | Base | Current | Δ | Alloc base | Alloc now | Δ | Status |");
        sb.AppendLine("|---|---|---:|---:|---:|---:|---:|---:|---|");

        foreach (var row in report.Rows.OrderBy(r => r.Status).ThenBy(r => r.Id, StringComparer.Ordinal))
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
                    sb.AppendLine("| Scenario | Median | p95 | Working set | Managed heap | Realized | n |");
                    sb.AppendLine("|---|---:|---:|---:|---:|---:|---:|");
                    foreach (var e in tier.OrderBy(e => e.Id, StringComparer.Ordinal))
                    {
                        sb.Append("| `").Append(e.Id).Append("` | ");
                        if (e.Error is not null) { sb.Append("error: ").Append(e.Error.Replace('|', '/')).AppendLine(" | | | | | |"); continue; }
                        sb.Append(Time(e.Time?.Median, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.P95, e.Time?.Unit)).Append(" | ")
                          .Append(Bytes(e.Process?.WorkingSetBytes)).Append(" | ")
                          .Append(Bytes(e.Process?.ManagedHeapBytes)).Append(" | ")
                          .Append(e.Process is null ? "—" : e.Process.RealizedRows.ToString(CultureInfo.InvariantCulture)).Append(" | ")
                          .Append(e.N).AppendLine(" |");
                    }
                    break;

                default:
                    sb.AppendLine("| Mark | Median | p95 | Min | n |");
                    sb.AppendLine("|---|---:|---:|---:|---:|");
                    foreach (var e in tier.OrderBy(e => e.Time?.Median ?? double.MaxValue).ThenBy(e => e.Id, StringComparer.Ordinal))
                    {
                        sb.Append("| `").Append(e.Id).Append("` | ");
                        if (e.Error is not null) { sb.Append("error: ").Append(e.Error.Replace('|', '/')).AppendLine(" | | | |"); continue; }
                        sb.Append(Time(e.Time?.Median, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.P95, e.Time?.Unit)).Append(" | ")
                          .Append(Time(e.Time?.Min, e.Time?.Unit)).Append(" | ")
                          .Append(e.N).AppendLine(" |");
                    }
                    break;
            }

            sb.AppendLine();
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
        CompareStatus.Slower => "slower",
        CompareStatus.Errored => "errored",
        CompareStatus.Removed => "removed",
        CompareStatus.New => "new",
        CompareStatus.AllocDown => "allocates less",
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

    private static string Pct(double pct) => pct.ToString("0.#", CultureInfo.InvariantCulture) + "%";

    /// <summary>Three significant figures, no exponent.</summary>
    private static string Num(double v)
    {
        var a = Math.Abs(v);
        var format = a >= 100 ? "0" : a >= 10 ? "0.0" : "0.00";
        return v.ToString(format, CultureInfo.InvariantCulture);
    }
}
