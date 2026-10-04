using System.ComponentModel;
using System.Diagnostics;
using System.Windows;
using System.Windows.Threading;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Benchmarking;

namespace BertBrowser.App.Services;

/// <summary>
/// Times a real launch from the inside, for <c>BertBrowser.Bench startup</c>.
/// </summary>
/// <remarks>
/// <para>
/// Inert unless <c>BERTBROWSER_STARTUP_TRACE</c> names a file, and then it does three things: records
/// marks as milliseconds since the process started, parks the window where the harness parks it so a
/// measurement run leaves the user's screen alone, and — only with <c>BERTBROWSER_EXIT_AFTER_STARTUP=1</c>
/// as well — shuts the app down once the first listing has been shown and rendered. Environment
/// variables rather than a flag, because <c>CommandLine</c> rejects options it does not know and a
/// user-facing switch for a benchmark would be a strange thing to document.
/// </para>
/// <para>
/// <b>A traced launch skips what follows the window.</b> It runs against a scratch data directory, and
/// the index helper's pipe is per user, not per data directory — so the normal attach would find the
/// user's real helper, and <c>ApplyChangeLogPolicy</c> would push the scratch settings' default of
/// "recording off" at it, which wipes the user's change log. The folder-handler repair, the update
/// check and the staging sweep are left out for the same reason: they act on the machine, not on the
/// scratch directory. All of them run after <c>Show()</c> and off the critical path, so "process start
/// to first listing" is still the real thing; the trace says <c>sideEffectsSkipped</c> so nobody reads
/// it as "to idle".
/// </para>
/// </remarks>
internal static class StartupTrace
{
    public const string FileVariable = "BERTBROWSER_STARTUP_TRACE";
    public const string ExitVariable = "BERTBROWSER_EXIT_AFTER_STARTUP";

    /// <summary>If the first listing never comes, write what there is and get out of the way.</summary>
    private static readonly TimeSpan Fallback = TimeSpan.FromSeconds(60);

    private static readonly string? TracePath =
        Environment.GetEnvironmentVariable(FileVariable) is { Length: > 0 } path ? Path.GetFullPath(path) : null;

    private static readonly object Gate = new();
    private static readonly Dictionary<string, double> Marks = new(StringComparer.Ordinal);
    private static Stopwatch? _clock;
    private static DateTime _processStartUtc;
    private static double _clockOffsetMs;
    private static string _instance = "unknown";
    private static bool _flushed;
    private static Timer? _fallback;

    public static bool Enabled => TracePath is not null;

    private static bool ExitAfter => Environment.GetEnvironmentVariable(ExitVariable) == "1";

    /// <summary>Records <paramref name="name"/> at now, once; a later repeat is ignored.</summary>
    public static void Mark(string name)
    {
        if (!Enabled) return;

        lock (Gate)
        {
            if (_clock is null)
            {
                // The gap from process start to the first mark is the OS's clock against ours — a
                // millisecond or so coarse, which is stated in the docs and is why `main` is reported
                // rather than used as a zero.
                using var process = Process.GetCurrentProcess();
                _processStartUtc = process.StartTime.ToUniversalTime();
                _clockOffsetMs = (DateTime.UtcNow - _processStartUtc).TotalMilliseconds;
                _clock = Stopwatch.StartNew();
            }

            Marks.TryAdd(name, _clockOffsetMs + _clock.Elapsed.TotalMilliseconds);
        }
    }

    public static void Instance(bool isFirst) => _instance = isFirst ? "first" : "second";

    /// <summary>
    /// Parks the window and arranges for <see cref="StartupMarks.ContentRendered"/> and
    /// <see cref="StartupMarks.FirstListing"/> to be recorded, after which the trace is written.
    /// Call before <c>Show()</c>.
    /// </summary>
    public static void Observe(Window window, ShellViewModel shell)
    {
        if (!Enabled) return;

        Park(window);

        window.ContentRendered += (_, _) =>
        {
            Mark(StartupMarks.ContentRendered);
            FinishIfComplete(window);
        };

        // The same signal the UI harness waits on: a tab with a path in it that is no longer loading.
        var tab = shell.ActiveTab;
        PropertyChangedEventHandler? onChanged = null;
        onChanged = (_, e) =>
        {
            if (e.PropertyName is not (nameof(FileListViewModel.IsLoading) or null)) return;
            if (tab.CurrentPath.Length == 0 || tab.FileList.IsLoading) return;

            tab.FileList.PropertyChanged -= onChanged;
            Mark(StartupMarks.FirstListing);
            FinishIfComplete(window);
        };
        tab.FileList.PropertyChanged += onChanged;

        _fallback = new Timer(
            _ => window.Dispatcher.BeginInvoke(() => Finish(window, "the first listing never arrived")),
            null, Fallback, Timeout.InfiniteTimeSpan);
    }

    /// <summary>Where the harness parks: off every monitor, never activated, not in the taskbar.</summary>
    private static void Park(Window window)
    {
        const int offscreen = -32000;
        window.WindowStartupLocation = WindowStartupLocation.Manual;
        window.ShowActivated = false;
        window.ShowInTaskbar = false;
        window.WindowState = WindowState.Normal;
        window.Left = offscreen;
        window.Top = offscreen;
        window.Width = 1400;
        window.Height = 900;
        window.Focusable = false;
    }

    private static void FinishIfComplete(Window window)
    {
        lock (Gate)
        {
            if (!Marks.ContainsKey(StartupMarks.ContentRendered) || !Marks.ContainsKey(StartupMarks.FirstListing)) return;
        }

        Finish(window, null);
    }

    private static void Finish(Window window, string? error)
    {
        StartupTraceData trace;
        lock (Gate)
        {
            if (_flushed) return;
            _flushed = true;
            _fallback?.Dispose();

            trace = new StartupTraceData(
                Environment.ProcessId,
                _processStartUtc,
                new Dictionary<string, double>(Marks),
                _instance,
                Indexer: "skipped",
                SideEffectsSkipped: true,
                error);
        }

        try
        {
            StartupTraceJson.Write(trace, TracePath!);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // A trace that cannot be written is a failed measurement, not a failed launch.
        }

        if (ExitAfter)
        {
            // ContextIdle, so the frame that fired ContentRendered has actually been presented.
            window.Dispatcher.BeginInvoke(DispatcherPriority.ContextIdle, () => Application.Current.Shutdown());
        }
    }
}
