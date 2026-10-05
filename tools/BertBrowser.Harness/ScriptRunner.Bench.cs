using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using System.Windows.Controls;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;

namespace BertBrowser.Harness;

/// <summary>
/// The benchmarking verbs: timing or repeating any other verb, reading the process's memory back, waiting for
/// the two things <c>Settle</c> deliberately does not wait for, and a fixture sized for quantity.
/// </summary>
/// <remarks>
/// <para>
/// <c>time</c> measures from issuing a verb to the window being quiet again, because every verb it
/// wraps already ends in <c>Settle</c> — that is the interval a person perceives, and it is why no
/// verb needed a stopwatch of its own. Several verbs may be chained with <c>;</c> inside one
/// <c>time</c>, so a selection and the wait for its preview are one sample.
/// </para>
/// <para>
/// Samples print as <c>BENCH</c>/<c>MEM</c> lines for a person and, with <c>--bench-out</c>, go to a
/// JSON-lines file for <c>BertBrowser.Bench ui</c>, which is the only thing that turns them into the
/// shared results schema. Nothing here decides what is fast.
/// </para>
/// </remarks>
internal sealed partial class ScriptRunner
{
    private const int DefaultThumbnailTimeoutMs = 30_000;

    /// <summary>
    /// The one sample that is not a verb: how long the session took to come up. Written first,
    /// before any command, so a script whose only purpose is this one needs no body.
    /// </summary>
    private void RecordHostedStartup()
    {
        if (options.BenchOut is null) return;
        RecordTime("startup.hosted", session.StartupMilliseconds);
    }

    /// <summary><c>time &lt;name&gt; &lt;verb&gt; [args][; &lt;verb&gt; [args]…]</c>.</summary>
    private void Time(string rest)
    {
        var (name, inner) = Split(Require(rest, "time"));
        if (inner.Length == 0) throw new FormatException("time wants '<name> <verb> [args]'.");

        var steps = inner.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (steps.Length == 0) throw new FormatException("time has nothing to run.");

        var clock = Stopwatch.StartNew();
        foreach (var step in steps) Execute(step);
        var ms = clock.Elapsed.TotalMilliseconds;

        RecordTime(name, ms);
    }

    /// <summary><c>repeat &lt;n&gt; &lt;verb&gt; [args][; &lt;verb&gt; [args]…]</c>: the chain, that many times over.</summary>
    /// <remarks>
    /// What a soak needs and nothing more: a leak is a few kilobytes a lap, invisible in one and
    /// unmissable in thirty. It owns the rest of its line, so it cannot sit inside a <c>time</c>
    /// chain — <c>time</c> splits on the same <c>;</c> first.
    /// </remarks>
    private void Repeat(string rest)
    {
        var (count, inner) = Split(Require(rest, "repeat"));
        var laps = Number(count, "repeat");
        var steps = inner.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (laps < 1 || steps.Length == 0) throw new FormatException("repeat wants '<n> <verb> [args][; <verb> [args]…]'.");

        for (var lap = 0; lap < laps; lap++)
            foreach (var step in steps) Execute(step);
    }

    /// <summary>What each named <c>mem</c> read, so a later <c>assert-mem … since</c> has something to subtract.</summary>
    private readonly Dictionary<string, MemorySample> _memorySamples = new(StringComparer.Ordinal);

    /// <summary><c>mem [name]</c>: the process after a full collection.</summary>
    private void Mem(string rest)
    {
        var name = rest.Length == 0 ? "mem" : rest;
        var sample = ReadMemory();
        _memorySamples[name] = sample;

        output.WriteLine("MEM " + JsonSerializer.Serialize(new
        {
            kind = "mem",
            name,
            sample.ManagedBytes,
            sample.WorkingSetBytes,
            sample.PrivateBytes,
            sample.PeakWorkingSetBytes,
            sample.Items,
            sample.Realized,
            sample.RetainedThumbnails,
        }, JsonLines));
        Append(new { kind = "mem", name, sample.ManagedBytes, sample.WorkingSetBytes, sample.PrivateBytes, sample.PeakWorkingSetBytes, sample.Items, sample.Realized, sample.RetainedThumbnails });
    }

    /// <summary>
    /// <c>assert-mem managed|private|workingset under &lt;mb&gt; [since &lt;name&gt;]</c>.
    /// </summary>
    /// <remarks>
    /// With <c>since</c> the budget is on <em>growth</em> over an earlier <c>mem &lt;name&gt;</c> — the
    /// shape a leak check takes, because what a soak may hold depends on the machine and what it
    /// may gain does not.
    /// </remarks>
    private void AssertMem(string rest)
    {
        const string usage = "assert-mem wants '<managed|private|workingset> under <mb> [since <name>]'.";
        var parts = Require(rest, "assert-mem").Split(' ', 5, StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length is not (3 or 5) || !parts[1].Equals("under", StringComparison.OrdinalIgnoreCase))
            throw new FormatException(usage);
        if (parts.Length == 5 && !parts[3].Equals("since", StringComparison.OrdinalIgnoreCase))
            throw new FormatException(usage);

        var limitMb = Number(parts[2], "assert-mem");
        var sample = ReadMemory();
        Func<MemorySample, long> read;
        string what;
        switch (parts[0].ToLowerInvariant())
        {
            case "managed": what = "managed heap"; read = s => s.ManagedBytes; break;
            case "private": what = "private bytes"; read = s => s.PrivateBytes; break;
            case "workingset": what = "working set"; read = s => s.WorkingSetBytes; break;
            default: throw new FormatException($"assert-mem: expected 'managed', 'private' or 'workingset', got '{parts[0]}'.");
        }

        if (parts.Length == 3)
        {
            var mb = read(sample) / (1024.0 * 1024);
            if (mb >= limitMb)
                throw new AssertionException($"expected the {what} under {limitMb} MB, got {mb:0.0} MB.");
            return;
        }

        if (!_memorySamples.TryGetValue(parts[4], out var earlier))
            throw new FormatException($"assert-mem: no 'mem {parts[4]}' has run yet.");

        var grownMb = (read(sample) - read(earlier)) / (1024.0 * 1024);
        if (grownMb >= limitMb)
        {
            throw new AssertionException(
                $"expected the {what} to have grown by under {limitMb} MB since '{parts[4]}', " +
                $"it grew {grownMb:0.0} MB (to {read(sample) / (1024.0 * 1024):0.0} MB).");
        }
    }

    /// <summary>
    /// <c>settle-thumbnails [ms]</c>: waits until no icon or thumbnail request is queued behind or
    /// inside the shell gates, then settles.
    /// </summary>
    /// <remarks>
    /// A verb rather than part of <c>Settle</c>: folding the gates into <c>IsBusy</c> would make every
    /// script wait on shell COM calls it has no interest in. Only a tile scenario cares that every
    /// visible tile has heard back, and it says so.
    /// </remarks>
    private void SettleThumbnails(string rest)
    {
        var timeout = rest.Length == 0 ? DefaultThumbnailTimeoutMs : Number(rest, "settle-thumbnails");
        var clock = Stopwatch.StartNew();

        while (clock.ElapsedMilliseconds < timeout)
        {
            session.Dispatcher.Invoke(() => { }, DispatcherPriority.Background);
            if (FileItemViewModel.PendingThumbnails == 0 && FileItemViewModel.PendingIcons == 0) break;
            Thread.Sleep(5);
        }

        if (FileItemViewModel.PendingThumbnails != 0 || FileItemViewModel.PendingIcons != 0)
            throw new TimeoutException($"{FileItemViewModel.PendingThumbnails} thumbnail(s) and {FileItemViewModel.PendingIcons} icon(s) were still pending after {timeout} ms.");

        session.Settle();
        output.WriteLine($"SETTLED-THUMBS {clock.Elapsed.TotalMilliseconds:0}");
    }

    /// <summary>
    /// <c>settle-preview</c>: waits for the preview pane's debounced load to start and finish.
    /// </summary>
    /// <remarks>
    /// The pane debounces a selection before reading, so a plain <c>Settle</c> after <c>select</c>
    /// returns with the previous body still showing. The wait is on <c>IsLoadPending</c>, which is
    /// up from the moment the load is scheduled: watching <c>IsLoading</c> rise and fall instead
    /// missed any read short enough to do both between two looks, and then sat out a whole
    /// start window waiting for a rise that had already happened.
    /// </remarks>
    private void SettlePreview()
    {
        var clock = Stopwatch.StartNew();
        var preview = session.Tab.Preview;

        while (clock.ElapsedMilliseconds < options.BusyTimeoutMs && session.Dispatcher.Invoke(() => preview.IsLoadPending))
            session.Dispatcher.Invoke(() => { }, DispatcherPriority.Background);

        if (session.Dispatcher.Invoke(() => preview.IsLoadPending))
            throw new TimeoutException($"The preview was still loading after {options.BusyTimeoutMs} ms.");

        session.Settle();
    }

    /// <summary>
    /// <c>bench-fixture &lt;files&gt; &lt;dirs&gt; [dir]</c>: that many empty files, straight into the folder when
    /// <c>dirs</c> is 0 or spread round-robin over that many subfolders.
    /// </summary>
    /// <remarks>
    /// Empty, because what the listing and the transfer scenarios measure is per-entry work, not
    /// bytes; and named by index so the search scenario can pick a substring with a known hit count.
    /// Every fourth file is a <c>.txt</c>, the mix the tile view lays out as a row among tiles.
    /// <c>many-fixture</c> stays as it is: its <c>.mp4</c> names are what make the tile scenario a
    /// tile scenario.
    /// </remarks>
    private void BenchFixture(string rest)
    {
        var parts = Require(rest, "bench-fixture").Split(' ', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length is < 2 or > 3)
            throw new FormatException("bench-fixture wants '<files> <dirs> [dir]'.");

        var files = Number(parts[0], "bench-fixture");
        var dirs = Number(parts[1], "bench-fixture");
        var root = _sandbox.RequireInside(parts.Length == 3 ? parts[2] : "Bench", "bench-fixture");
        Directory.CreateDirectory(root);

        string[] cycle = [".jpg", ".png", ".mp4", ".txt", ".cs", ".json", ".xml", ".txt"];
        for (var d = 0; d < dirs; d++)
            Directory.CreateDirectory(Path.Combine(root, $"D{d:000}"));

        for (var i = 0; i < files; i++)
        {
            var folder = dirs == 0 ? root : Path.Combine(root, $"D{i % dirs:000}");
            using var _ = File.Create(Path.Combine(folder, $"f{i:00000}{cycle[i % cycle.Length]}"));
        }

        Sandbox.Stamp(root);
        output.WriteLine($"# bench fixture: {files} files in {Math.Max(dirs, 1)} folder(s) under {root}");
    }

    // ---- readback ----

    private readonly record struct MemorySample(
        long ManagedBytes, long WorkingSetBytes, long PrivateBytes, long PeakWorkingSetBytes,
        int Items, int Realized, int RetainedThumbnails);

    private MemorySample ReadMemory() => session.Dispatcher.Invoke(() =>
    {
        // Twice, with finalizers run between: the first pass queues the finalizable objects (bitmaps,
        // SQLite handles), the second collects what they released. What is left is what is held.
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();

        using var process = Process.GetCurrentProcess();
        process.Refresh();

        var list = session.Tab.FileList;
        return new MemorySample(
            GC.GetTotalMemory(forceFullCollection: false),
            process.WorkingSet64,
            process.PrivateMemorySize64,
            process.PeakWorkingSet64,
            list.Items.Count,
            RealizedRowCount(),
            list.RetainedThumbnailCount);
    });

    /// <summary>How many rows the list has actually built, or -1 when it has no virtualizing panel.</summary>
    private int RealizedRowCount() => session.Dispatcher.Invoke(() =>
    {
        var list = FindNamed<ListView>("FileListView");
        if (list is null) return -1;
        return VisualTreeUtil.FindDescendant<VirtualizingPanel>(list) is { } panel
            ? panel.Children.Count
            : -1;
    });

    // ---- recording ----

    private static readonly JsonSerializerOptions JsonLines = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    };

    private void RecordTime(string name, double ms)
    {
        output.WriteLine($"BENCH {name} {ms.ToString("0.0", CultureInfo.InvariantCulture)}");
        Append(new { kind = "time", name, ms });
    }

    private void Append(object sample)
    {
        if (options.BenchOut is null) return;

        if (Path.GetDirectoryName(options.BenchOut) is { Length: > 0 } dir) Directory.CreateDirectory(dir);
        File.AppendAllText(options.BenchOut, JsonSerializer.Serialize(sample, JsonLines) + "\n");
    }
}
