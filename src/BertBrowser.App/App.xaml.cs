using System.Windows;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Cli;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Mft;
using Microsoft.Extensions.DependencyInjection;
using Velopack;

namespace BertBrowser.App;

public partial class App : Application
{
    public static IServiceProvider Services { get; private set; } = null!;

    private static SingleInstance? _instance;

    [STAThread]
    private static void Main(string[] args)
    {
        // Must run before any WPF code: handles Velopack install/update/uninstall
        // hooks and exits the process when invoked as one.
        VelopackApp.Build().Run();

        // After Velopack, whose hooks exit the process and must not be gated behind an instance
        // check — and before anything else, so a second launch costs no WPF, no DI and no database.
        _instance = SingleInstance.Claim();
        if (!_instance.IsFirst && _instance.TryHandOff(CommandLine.Parse(args)))
        {
            _instance.Dispose();
            return;
        }

        // Either we are the first copy, or the first one could not be reached — it may be
        // mid-shutdown — in which case starting normally beats exiting and doing nothing at all.
        var app = new App();
        app.InitializeComponent();
        app.Run();
    }

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        AppPaths.MigrateLegacyData();

        Services = BuildServices();

        Services.GetRequiredService<Db>().Migrate();

        // Before any window exists, so the first frame is already in the chosen theme.
        Services.GetRequiredService<IThemeService>().Initialize();

        // Start path priority: command-line argument, then last visited, then user profile.
        var settings = Services.GetRequiredService<AppSettings>();
        var shell = Services.GetRequiredService<ShellViewModel>();
        var startup = CommandLine.Parse(e.Args);
        if (startup.Targets.FirstOrDefault(t => Directory.Exists(t.Path)) is { } target)
            shell.StartPath = target.Path;
        else if (settings.LastPath is { } last && Directory.Exists(last))
            shell.StartPath = last;

        var window = Services.GetRequiredService<MainWindow>();
        window.Show();

        // Anything the first target could not cover — extra paths, /select, --new-tab — once there
        // is a window to open it in.
        if (startup.Targets.Count > 0)
            _ = shell.OpenRequestAsync(RemainingAfterStart(startup, shell.StartPath));

        ListenForOtherInstances(window, shell);

        // Build the global MFT search index in the background (each NTFS volume on its own
        // thread); it needs the elevation this app requests via its manifest.
        Services.GetRequiredService<IMftIndexService>().Start();

        _ = Task.Run(() => Services.GetRequiredService<IUpdateService>().CheckAndStageUpdateAsync());

        // A delete holds its items until the undo record is retired, which normally happens by the
        // time the app closes. A crash leaves them behind, so sweep up anything a previous session
        // abandoned — only batches over a day old, so a second copy running right now keeps its
        // pending undo.
        _ = Task.Run(() => BertBrowser.Core.Services.Delete.DeleteExecutor
            .PurgeAbandonedStaging(TimeSpan.FromDays(1)));
    }

    /// <summary>
    /// The composition root, and nothing else: building the graph starts no indexer, opens no
    /// window and writes to no disk.
    /// </summary>
    /// <remarks>
    /// Separate from <see cref="OnStartup"/> because the UI harness hosts the same window in a
    /// process that never calls <c>Application.Run</c>, and needs the same services without the
    /// side effects that belong to a real launch — the MFT indexer, the update check, the staging
    /// sweep and the single-instance listener. <paramref name="customize"/> runs last, so a caller
    /// can replace a registration (the harness swaps <see cref="IProcessLauncher"/> for one that
    /// refuses, since a scripted run must not start programs on the user's desktop).
    /// </remarks>
    internal static IServiceProvider BuildServices(Action<IServiceCollection>? customize = null)
    {
        var services = new ServiceCollection();
        services.AddSingleton(AppSettings.Load());
        services.AddSingleton<UserThemeStore>();
        services.AddSingleton<IThemeService, ThemeService>();
        services.AddSingleton(new Db(AppPaths.DbPath));
        services.AddSingleton<DirSizeRepository>();
        services.AddSingleton<FsIndexRepository>();
        services.AddSingleton<BookmarkRepository>();
        services.AddSingleton<IFileSystemService, FileSystemService>();
        services.AddSingleton<IFileTransferService, FileTransferService>();
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferPlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferExecutor>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenamePlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenameExecutor>();
        // One instance serving both roles: it caches per-volume answers, and the planner and the
        // executor should agree about what has a Recycle Bin.
        services.AddSingleton<Interop.ShellRecycleBin>();
        services.AddSingleton<BertBrowser.Core.Services.Delete.IRecycleBin>(
            s => s.GetRequiredService<Interop.ShellRecycleBin>());
        services.AddSingleton<BertBrowser.Core.Services.Delete.IRecycleProbe>(
            s => s.GetRequiredService<Interop.ShellRecycleBin>());
        services.AddSingleton(s => new BertBrowser.Core.Services.Delete.DeletePlanner(
            new BertBrowser.Core.Services.Delete.FileSystemDeleteProbe(),
            protectedPaths: null,
            s.GetRequiredService<BertBrowser.Core.Services.Delete.IRecycleProbe>()));
        services.AddSingleton(s => new BertBrowser.Core.Services.Delete.DeleteExecutor(
            new BertBrowser.Core.Services.Delete.FileSystemDeleteProbe(),
            protectedPaths: null,
            stagingRoot: null,
            s.GetRequiredService<BertBrowser.Core.Services.Delete.IRecycleBin>(),
            s.GetRequiredService<BertBrowser.Core.Services.Delete.IRecycleProbe>()));
        services.AddSingleton<BertBrowser.Core.Services.Delete.DeleteSurveyor>();
        services.AddSingleton<IBookmarkService, BookmarkService>();
        services.AddSingleton<IndexCrawler>();
        services.AddSingleton<IIndexWatcherService, IndexWatcherService>();
        services.AddSingleton<IMftIndexService, MftIndexService>();
        services.AddSingleton<ISearchService, SearchService>();
        services.AddSingleton<IUpdateService, UpdateService>();
        services.AddSingleton<IProcessLauncher, ProcessLauncher>();
        services.AddSingleton<PaneFactory>();
        services.AddSingleton<ShellViewModel>();
        services.AddSingleton<MainWindow>();

        customize?.Invoke(services);

        return services.BuildServiceProvider();
    }

    /// <summary>Adopts a service graph built outside <see cref="OnStartup"/>, so the code-behind
    /// that reaches for <see cref="Services"/> works in a harness-hosted window too.</summary>
    internal static void UseServices(IServiceProvider services) => Services = services;

    /// <summary>
    /// The startup path already opened the first browsable target in the window's own first tab, so
    /// re-opening it would leave a duplicate. Everything else still needs opening — and a target
    /// asking for <c>/select</c> does, even if it is the one that set the start path, because the
    /// start path alone cannot carry "highlight this".
    /// </summary>
    private static CommandLineRequest RemainingAfterStart(CommandLineRequest request, string? startPath)
    {
        var remaining = request.Targets
            .Where(t => t.Select ||
                !string.Equals(t.Path, startPath, StringComparison.OrdinalIgnoreCase))
            .ToList();

        return request with { Targets = remaining };
    }

    /// <summary>
    /// Lets a second launch hand its command line here instead of starting a whole second copy —
    /// which would mean a second MFT indexer against the same database.
    /// </summary>
    private static void ListenForOtherInstances(Window window, ShellViewModel shell)
    {
        if (_instance is not { IsFirst: true } instance) return;

        instance.RequestReceived += request => window.Dispatcher.BeginInvoke(() =>
        {
            // Restore first: the request is worthless if the window it opens in stays minimized
            // behind whatever the user was actually looking at.
            if (window.WindowState == WindowState.Minimized)
                window.WindowState = WindowState.Normal;
            window.Activate();

            _ = shell.OpenRequestAsync(request);
        });

        instance.StartListening();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        // Stops the listener and releases the instance claim, so the next launch starts cleanly
        // rather than trying to hand off to a process that is on its way out.
        _instance?.Dispose();
        _instance = null;

        // Disposes IDisposable singletons (index watchers, search service).
        (Services as IDisposable)?.Dispose();
        base.OnExit(e);
    }
}
