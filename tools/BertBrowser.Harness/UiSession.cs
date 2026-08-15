using System.Diagnostics;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Threading;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Data;
using Microsoft.Extensions.DependencyInjection;
using AppShell = BertBrowser.App.App;

namespace BertBrowser.Harness;

/// <summary>
/// The real browser window, hosted where the user cannot see it and cannot be interrupted by it.
/// </summary>
/// <remarks>
/// <para>
/// The window is shown, laid out and rendered exactly as in production — it is simply parked
/// outside every monitor's coordinate space and refused activation. That matters because
/// <see cref="System.Windows.Media.Imaging.RenderTargetBitmap"/> re-renders the visual tree through
/// the software rasteriser rather than grabbing the screen, so where the window sits, what covers
/// it, and whether the compositor ever presents it are all irrelevant to what a capture contains.
/// </para>
/// <para>
/// Nothing here synthesises operating-system input. Commands run the same view-model methods the
/// buttons and key handlers run, so the app under test is the app the user runs, but the actions
/// exist only inside this process and cannot land in whatever the user is typing into.
/// </para>
/// </remarks>
internal sealed class UiSession : IDisposable
{
    private readonly HarnessOptions _options;
    private readonly ForegroundGuard _guard;

    private UiSession(
        HarnessOptions options,
        MainWindow window,
        IServiceProvider services,
        RefusingProcessLauncher launcher,
        ForegroundGuard guard)
    {
        _options = options;
        Window = window;
        Services = services;
        Launcher = launcher;
        _guard = guard;
    }

    public MainWindow Window { get; }

    public IServiceProvider Services { get; }

    /// <summary>What the run was asked to start, and refused to.</summary>
    public RefusingProcessLauncher Launcher { get; }

    public ShellViewModel Shell => (ShellViewModel)Window.DataContext;

    public DirectoryTabViewModel Tab => Shell.ActiveTab;

    public Dispatcher Dispatcher => Window.Dispatcher;

    /// <summary>How many times the window had to be pushed back out of the foreground.</summary>
    public int ForegroundCorrections => _guard.Corrections;

    /// <summary>Where the window was parked. Far enough out that no monitor arrangement reaches it.</summary>
    private const int Offscreen = -32000;

    /// <summary>
    /// Boots WPF, the service graph and the window, on the calling STA thread.
    /// </summary>
    public static UiSession Start(HarnessOptions options, TextWriter log)
    {
        // Set before anything touches the database: AppPaths reads it when its static initialiser
        // runs, so a run cannot inherit the user's search index, settings or themes — nor write to
        // them.
        Environment.SetEnvironmentVariable(AppPaths.OverrideVariable, options.StateDir);
        Directory.CreateDirectory(options.StateDir);
        Directory.CreateDirectory(options.SandboxDir);

        if (!AppPaths.DataDir.Equals(options.StateDir, StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException(
                $"The app resolved its data directory to '{AppPaths.DataDir}' rather than the scratch " +
                $"'{options.StateDir}'. Something read AppPaths before the override was set; refusing " +
                "to run against the user's real database.");

        // A window nobody presents has no use for a GPU, and the software rasteriser is what
        // captures go through anyway. Taking the hardware path out removes driver and render-tier
        // variance from the pictures.
        RenderOptions.ProcessRenderMode = RenderMode.SoftwareOnly;

        // InitializeComponent merges Theme.xaml and Styles.xaml at application scope exactly as
        // production does. Run() is what would call OnStartup — the MFT indexer, the update check,
        // the abandoned-staging sweep, the single-instance listener and a visible window all live
        // there, and none of it should happen here.
        var app = new AppShell();
        app.InitializeComponent();

        if (Application.Current.TryFindResource(BertBrowser.Core.Theming.ThemeToken.WindowBackground) is null)
            throw new InvalidOperationException(
                "The theme dictionary did not load at application scope; every capture would show " +
                "an unstyled window.");

        var launcher = new RefusingProcessLauncher();
        var services = AppShell.BuildServices(s => s.AddSingleton<IProcessLauncher>(launcher));
        AppShell.UseServices(services);

        services.GetRequiredService<Db>().Migrate();

        var themes = services.GetRequiredService<IThemeService>();
        themes.Initialize();
        if (options.ThemeId is { Length: > 0 } themeId)
            themes.SelectTheme(themeId);

        var shell = services.GetRequiredService<ShellViewModel>();
        shell.StartPath = options.StartPath ?? options.SandboxDir;

        var before = Native.Foreground();
        if (options.Verbose) log.WriteLine($"# foreground on entry: {before.Describe()}");

        // Started before the window exists, so the one activation that does happen — during the
        // first layout pass, through no event this process is told about — is corrected within a
        // tick rather than lasting until the run settles.
        var guard = ForegroundGuard.Start(before.Handle, log);

        var window = services.GetRequiredService<MainWindow>();
        Park(window, options);
        window.Activated += (_, _) => Native.RestoreForeground(before.Handle);
        window.Show();

        var session = new UiSession(options, window, services, launcher, guard);

        // The MFT indexer is off unless asked for: it reads every NTFS volume's master file table,
        // which is minutes of disk on a machine someone is using, and it needs rights this process
        // deliberately does not request.
        if (options.Index)
            services.GetRequiredService<BertBrowser.Core.Services.Mft.IMftIndexService>().Start();

        session.WaitForFirstListing();

        var after = Native.Foreground();
        if (after.Handle != before.Handle)
            log.WriteLine($"# WARNING: the foreground is {after.Describe()}, not where it started.");

        return session;
    }

    /// <summary>
    /// Puts a window where it will never be seen and can never be activated.
    /// </summary>
    /// <remarks>
    /// Shared with the dialogs, which get the same treatment: they are owned by a window that is
    /// itself offscreen, but <c>CenterOwner</c> would still place them relative to a parent at
    /// -32000 only by accident, and one of them setting its own position would be enough to put it
    /// on the user's screen.
    /// </remarks>
    public static void Park(Window window, HarnessOptions options, bool sizeIt = true)
    {
        window.WindowStartupLocation = WindowStartupLocation.Manual;
        window.ShowActivated = false;
        window.ShowInTaskbar = false;
        window.Left = Offscreen;
        window.Top = Offscreen;

        if (sizeIt)
        {
            // Explicit, so a picture does not depend on the bounds some earlier session saved —
            // and normal, so the maximise clamp in ThemedWindow never has to answer for a window
            // that is on no monitor.
            window.WindowState = WindowState.Normal;
            window.Width = options.WindowWidth;
            window.Height = options.WindowHeight;
        }

        // The file list focuses itself whenever a listing arrives, which is a SetFocus, which
        // activates a window even when it is disabled. Refusing focus at the WPF level keeps it
        // from getting that far.
        window.Focusable = false;
        window.SourceInitialized += (_, _) => Native.MakeNonInteractive(window);
    }

    /// <summary>
    /// Waits out the startup the window kicks off from <c>Loaded</c>.
    /// </summary>
    /// <remarks>
    /// <c>ShellViewModel.InitializeAsync</c> loads bookmarks, enumerates drives and navigates the
    /// first tab, and none of that has begun when <c>Show</c> returns — so a <see cref="Settle"/>
    /// straight after it would find nothing loading and come back immediately, and the first
    /// capture would be of an empty window. The signal that it finished is a tab with a path in it.
    /// </remarks>
    private void WaitForFirstListing()
    {
        var clock = Stopwatch.StartNew();

        while (clock.ElapsedMilliseconds < _options.BusyTimeoutMs)
        {
            Dispatcher.Invoke(() => { }, DispatcherPriority.Background);
            if (Tab.CurrentPath.Length > 0 && !Tab.FileList.IsLoading) break;
        }

        Settle();
    }

    /// <summary>
    /// Runs the app forward until it has nothing left to do.
    /// </summary>
    /// <remarks>
    /// <para>
    /// <c>FileListViewModel.IsLoading</c> brackets every listing and every search, and the disk
    /// enumeration behind them outlasts any number of empty dispatcher passes; <c>IsTransferring</c>
    /// does the same for a move. Pumping at <see cref="DispatcherPriority.Background"/> is what lets
    /// the continuations waiting on the captured synchronization context run at all.
    /// </para>
    /// <para>
    /// The final pass at <see cref="DispatcherPriority.ContextIdle"/> returns only once layout and
    /// render have been through, which is the state a capture should see.
    /// </para>
    /// </remarks>
    public void Settle(int quietMs = 0)
    {
        var clock = Stopwatch.StartNew();

        while (IsBusy && clock.ElapsedMilliseconds < _options.BusyTimeoutMs)
            Dispatcher.Invoke(() => { }, DispatcherPriority.Background);

        if (IsBusy)
            throw new TimeoutException($"The window was still busy after {_options.BusyTimeoutMs} ms.");

        if (quietMs > 0)
        {
            var quiet = Stopwatch.StartNew();
            while (quiet.ElapsedMilliseconds < quietMs)
            {
                Dispatcher.Invoke(() => { }, DispatcherPriority.Background);
                Thread.Sleep(5);
            }
        }

        Window.UpdateLayout();
        Dispatcher.Invoke(() => { }, DispatcherPriority.ContextIdle);
    }

    /// <summary>Every open tab, not just the visible one: a background tab reloading is still work
    /// in flight, and a capture taken over it would catch the list mid-replacement.</summary>
    private bool IsBusy => Dispatcher.Invoke(() =>
        Shell.IsTransferring || Shell.AllTabs.Any(t => t.FileList.IsLoading));

    /// <summary>
    /// Waits for a debounced search to have been issued and finished.
    /// </summary>
    /// <remarks>
    /// Typing into either search box restarts a 200 ms timer rather than searching, so a plain
    /// <see cref="Settle"/> would return before the search had even started and report the previous
    /// listing. Waiting a fixed 260 ms instead is what a flake is made of — it passed nine times in
    /// ten and once came back with an empty list. So this waits for the search to have *started*,
    /// which <c>FileListViewModel.BeginSearch</c> announces, and then lets <see cref="Settle"/>
    /// wait for it to finish.
    /// </remarks>
    public void SettleSearch()
    {
        var clock = Stopwatch.StartNew();

        // A query the parser rejects — one character, or nothing but wildcards — never starts a
        // search at all, so this gives up rather than failing: the listing it leaves behind is the
        // right answer for that case, and the assertions are what should say so.
        while (clock.ElapsedMilliseconds < 2000)
        {
            Dispatcher.Invoke(() => { }, DispatcherPriority.Background);
            if (Dispatcher.Invoke(() => Tab.FileList.IsLoading)) break;

            Thread.Sleep(5);
        }

        Settle();
    }

    public void Dispose()
    {
        // Closing rather than abandoning, so the Closing handler runs: it retires the undo slot,
        // which is what commits whatever a Replace or a delete was still holding — into the scratch
        // sandbox, which is the only place this run has written.
        try
        {
            Dispatcher.Invoke(Window.Close);
        }
        catch (Exception e) when (e is InvalidOperationException or TaskCanceledException)
        {
            // The dispatcher is already going down; nothing left worth saving.
        }

        // Disposes the singletons that hold file watchers and background threads — the search
        // service and the index watchers — exactly as App.OnExit does.
        (Services as IDisposable)?.Dispose();

        // Db pools its connections, so the scratch database stays open (and undeletable) until the
        // pool lets go. Without this every run leaves its state directory behind.
        Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();

        Dispatcher.InvokeShutdown();
        _guard.Dispose();
    }
}
