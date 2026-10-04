namespace BertBrowser.Core.Benchmarking;

/// <summary>How strict a comparison is.</summary>
/// <param name="TimeThresholdPct">A median this much slower than the baseline is a regression.</param>
/// <param name="TimeNoiseFloorNs">For nanosecond results, a slowdown smaller than this in absolute
/// terms is never a regression, whatever the percentage — a 2 ns benchmark that reads 3 ns is noise.</param>
/// <param name="TimeNoiseFloorMs">The same floor for millisecond results.</param>
/// <param name="AllocTolerancePctOverride">Replaces every entry's own <see cref="AllocStats.TolerancePct"/> when set.</param>
/// <param name="GateTime">Whether a slower median counts towards <see cref="CompareReport.TimeRegressed"/>. Off in CI.</param>
/// <param name="GateAlloc">Whether more allocation counts towards <see cref="CompareReport.AllocRegressed"/>.</param>
/// <param name="Strict">Whether a benchmark present in the baseline but missing now is a failure.</param>
/// <param name="MemoryThresholdPct">A process snapshot whose managed heap grew by more than this is a
/// regression.</param>
/// <param name="MemoryNoiseFloorBytes">Managed-heap growth smaller than this in absolute terms is
/// never a regression.</param>
/// <param name="PrivateBytesThresholdPct">The same threshold for private bytes, and much the wider:
/// measured over identical runs the managed heap after a full collection repeats to a tenth of a
/// megabyte, while private bytes — native heaps, the rasteriser, memory the collector has not handed
/// back — moved by ten megabytes in a hundred.</param>
/// <param name="PrivateBytesNoiseFloorBytes">The floor that goes with it.</param>
/// <param name="GateMemory">Whether a bigger process counts towards <see cref="CompareReport.MemoryRegressed"/>. Off in CI.</param>
public sealed record CompareOptions(
    double TimeThresholdPct = 15,
    double TimeNoiseFloorNs = 200,
    double TimeNoiseFloorMs = 5,
    double? AllocTolerancePctOverride = null,
    bool GateTime = true,
    bool GateAlloc = true,
    bool Strict = false,
    double MemoryThresholdPct = 10,
    long MemoryNoiseFloorBytes = 1L * 1024 * 1024,
    bool GateMemory = true,
    double PrivateBytesThresholdPct = 25,
    long PrivateBytesNoiseFloorBytes = 16L * 1024 * 1024);

/// <summary>What a comparison concluded about one benchmark. Ordered worst first.</summary>
public enum CompareStatus
{
    AllocUp,
    MemoryUp,
    Slower,
    Errored,
    Removed,
    New,
    AllocDown,
    MemoryDown,
    Faster,
    Same,
}

/// <summary>One benchmark, baseline against current.</summary>
public sealed record CompareRow(
    string Id,
    string Tier,
    CompareStatus Status,
    double? BaselineMedian,
    double? CurrentMedian,
    string? Unit,
    double? TimeDeltaPct,
    long? BaselineAlloc,
    long? CurrentAlloc,
    double? AllocDeltaPct,
    string? Note,
    ProcessStats? BaselineProcess = null,
    ProcessStats? CurrentProcess = null,
    double? MemoryDeltaPct = null);

/// <summary>
/// The whole verdict. The flags are what the gates read; the rows are what a person reads.
/// </summary>
/// <remarks>
/// A row's <see cref="CompareRow.Status"/> says what was observed, whichever gates are on — a slower
/// median is <see cref="CompareStatus.Slower"/> in CI too, where it is reported and not gated. Only the
/// flags honour <see cref="CompareOptions.GateTime"/>, <see cref="CompareOptions.GateAlloc"/> and
/// <see cref="CompareOptions.GateMemory"/>.
/// </remarks>
public sealed record CompareReport(
    IReadOnlyList<CompareRow> Rows,
    bool TimeRegressed,
    bool AllocRegressed,
    bool HasErrors,
    bool HasRemoved,
    bool MemoryRegressed = false);

/// <summary>Judges one results file against a baseline.</summary>
public static class BenchCompare
{
    public static CompareReport Compare(BenchResultSet baseline, BenchResultSet current, CompareOptions options)
    {
        if (baseline.Run.Real != current.Run.Real)
        {
            throw new InvalidOperationException(
                "A run against real data can only be compared with a baseline recorded against real data, " +
                "and a synthetic run only with a synthetic baseline. The benchmark ids coincide; the corpora do not.");
        }

        var before = baseline.Benchmarks.ToDictionary(b => b.Id, StringComparer.Ordinal);
        var rows = new List<CompareRow>(current.Benchmarks.Count);
        var timeRegressed = false;
        var allocRegressed = false;
        var memoryRegressed = false;
        var hasErrors = false;

        foreach (var now in current.Benchmarks)
        {
            before.Remove(now.Id, out var was);

            if (now.Error is not null)
            {
                hasErrors = true;
                rows.Add(new CompareRow(now.Id, now.Tier, CompareStatus.Errored,
                    was?.Time?.Median, null, was?.Time?.Unit, null,
                    was?.Memory?.AllocatedBytes, null, null, now.Error, was?.Process));
                continue;
            }

            if (was is null)
            {
                rows.Add(new CompareRow(now.Id, now.Tier, CompareStatus.New,
                    null, now.Time?.Median, now.Time?.Unit, null,
                    null, now.Memory?.AllocatedBytes, null, null, null, now.Process));
                continue;
            }

            var (slower, faster, timeDelta) = JudgeTime(was.Time, now.Time, options);
            var (allocUp, allocDown, allocDelta) = JudgeAlloc(was.Memory, now.Memory, options);
            var (memoryUp, memoryDown, memoryDelta) = JudgeProcess(was.Process, now.Process, options);

            timeRegressed |= slower && options.GateTime;
            allocRegressed |= allocUp && options.GateAlloc;
            memoryRegressed |= memoryUp && options.GateMemory;

            var status = allocUp ? CompareStatus.AllocUp
                : memoryUp ? CompareStatus.MemoryUp
                : slower ? CompareStatus.Slower
                : allocDown ? CompareStatus.AllocDown
                : memoryDown ? CompareStatus.MemoryDown
                : faster ? CompareStatus.Faster
                : CompareStatus.Same;

            rows.Add(new CompareRow(now.Id, now.Tier, status,
                was.Time?.Median, now.Time?.Median, now.Time?.Unit ?? was.Time?.Unit, timeDelta,
                was.Memory?.AllocatedBytes, now.Memory?.AllocatedBytes, allocDelta, null,
                was.Process, now.Process, memoryDelta));
        }

        foreach (var gone in before.Values)
        {
            rows.Add(new CompareRow(gone.Id, gone.Tier, CompareStatus.Removed,
                gone.Time?.Median, null, gone.Time?.Unit, null,
                gone.Memory?.AllocatedBytes, null, null, "in the baseline, not in this run", gone.Process));
        }

        return new CompareReport(rows, timeRegressed, allocRegressed, hasErrors, before.Count > 0, memoryRegressed);
    }

    private static (bool Slower, bool Faster, double? DeltaPct) JudgeTime(
        TimeStats? was, TimeStats? now, CompareOptions o)
    {
        if (was is null || now is null || was.Median <= 0) return (false, false, null);

        var delta = now.Median - was.Median;
        var deltaPct = delta / was.Median * 100;
        var floor = now.Unit switch
        {
            "ns" => o.TimeNoiseFloorNs,
            "ms" => o.TimeNoiseFloorMs,
            _ => 0,
        };

        var slower = deltaPct > o.TimeThresholdPct && delta > floor;
        var faster = -deltaPct > o.TimeThresholdPct && -delta > floor;
        return (slower, faster, deltaPct);
    }

    private static (bool Up, bool Down, double? DeltaPct) JudgeAlloc(
        AllocStats? was, AllocStats? now, CompareOptions o)
    {
        if (was is null || now is null) return (false, false, null);

        var tolerance = o.AllocTolerancePctOverride ?? now.TolerancePct;
        var delta = now.AllocatedBytes - was.AllocatedBytes;
        double? deltaPct = was.AllocatedBytes == 0
            ? (delta == 0 ? 0 : null)
            : delta / (double)was.AllocatedBytes * 100;

        // A baseline of zero bytes is exact: any allocation at all is a regression, whatever the
        // tolerance says, because there is no number for a percentage to be of.
        var up = was.AllocatedBytes == 0
            ? delta > 0
            : now.AllocatedBytes > was.AllocatedBytes * (1 + tolerance / 100);
        var down = was.AllocatedBytes > 0 && now.AllocatedBytes < was.AllocatedBytes * (1 - tolerance / 100);

        return (up, down, deltaPct);
    }

    /// <summary>
    /// A process snapshot against its baseline. The managed heap and private bytes are each judged
    /// against their own threshold and floor, and either one over is "up"; the reported delta is
    /// private bytes'. The working set is never judged — Windows trims it when it likes, so it says
    /// as much about the machine's afternoon as about the app.
    /// </summary>
    private static (bool Up, bool Down, double? DeltaPct) JudgeProcess(
        ProcessStats? was, ProcessStats? now, CompareOptions o)
    {
        if (was is null || now is null) return (false, false, null);

        var privateBytes = Moved(was.PrivateBytes, now.PrivateBytes, o.PrivateBytesThresholdPct, o.PrivateBytesNoiseFloorBytes);
        var managed = Moved(was.ManagedHeapBytes, now.ManagedHeapBytes, o.MemoryThresholdPct, o.MemoryNoiseFloorBytes);
        double? deltaPct = was.PrivateBytes > 0
            ? (now.PrivateBytes - was.PrivateBytes) / (double)was.PrivateBytes * 100
            : null;

        var up = privateBytes > 0 || managed > 0;
        return (up, !up && (privateBytes < 0 || managed < 0), deltaPct);
    }

    /// <summary>+1 when <paramref name="now"/> is over the threshold and the floor, -1 when it is under both, else 0.</summary>
    private static int Moved(long was, long now, double thresholdPct, long floorBytes)
    {
        var delta = now - was;
        if (Math.Abs(delta) <= floorBytes) return 0;
        if (was <= 0) return Math.Sign(delta);
        return Math.Abs(delta) > was * thresholdPct / 100 ? Math.Sign(delta) : 0;
    }
}
