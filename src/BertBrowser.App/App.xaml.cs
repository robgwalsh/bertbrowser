using System.Windows;
using BertBrowser.App.Interop;
using BertBrowser.App.Services;
using BertBrowser.App.Services.Indexing;
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
        VelopackApp.Build()
            // Uninstalling deletes the whole install directory, so a folder-handler registration
            // left behind would point the Windows shell at an executable that is gone — and since
            // it owns the Directory and Drive open verbs, *every folder double-click on the
            // machine* would fail, with the registry as the only way back. This runs inside the
            // uninstall with a 30-second budget, which is ample for six registry deletes, and
            // deliberately touches nothing but the registry: the process exits straight after.
            .OnBeforeUninstallFastCallback(_ => FolderHandlerRegistry.TryUnregister())
            .Run();

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
        {
            // Someone who named a folder on the command line asked for that folder. Reopening last
            // session's panes over the top of it would bury the thing they asked for.
            shell.StartPath = target.Path;
        }
        else
        {
            // Pruned here rather than inside the shell so "is this still a directory?" stays with
            // the caller, the way the rest of the startup path already decides that. The predicate
            // accepts somewhere inside an archive too — closing the app in a zip and losing the tab
            // is worse than reopening on a banner if the container has since gone.
            shell.StartLayout = BertBrowser.Core.Layout.SessionLayoutRules.Prune(
                settings.Session,
                path => Directory.Exists(path) ||
                        BertBrowser.Core.Services.Archives.ArchivePath.Parse(path, File.Exists) is not null);

            if (settings.LastPath is { } last && Directory.Exists(last))
                shell.StartPath = last;
        }

        var window = Services.GetRequiredService<MainWindow>();
        window.Show();

        // Anything the first target could not cover — extra paths, /select, --new-tab — once there
        // is a window to open it in.
        if (startup.Targets.Count > 0)
            _ = shell.OpenRequestAsync(RemainingAfterStart(startup, shell.StartPath));

        ListenForOtherInstances(window, shell);

        // The backstop behind the uninstall hook: if this app owns the shell's folder verb and the
        // registration has gone stale — an install moved, a write interrupted part-way — put a live
        // path back. Narrow on purpose: it never creates a registration that is absent and never
        // touches one belonging to another program. See FolderHandlerRules.ShouldRepair.
        FolderHandlerRegistry.RepairIfStale();

        // Build the global MFT search index in the background. This is what raises the one
        // elevation prompt the app asks for: reading the MFT needs an administrator token, so it
        // happens in BertBrowser.Indexer.exe rather than here. Declining costs instant global
        // search and nothing else — SearchService falls back to its crawl, and the status bar
        // offers a retry.
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
        // The archive layer is a decorator, which is why the five callers of IFileSystemService —
        // the file list, its merge diff, the disk-usage breakdown and the folder tree — needed no
        // changes at all to gain it. The concrete FileSystemService is registered separately so the
        // decorator has something real to fall through to.
        services.AddSingleton<FileSystemService>();
        services.AddSingleton<BertBrowser.Core.Services.Archives.IArchiveReader,
            BertBrowser.Core.Services.Archives.SharpCompressArchiveReader>();
        services.AddSingleton<BertBrowser.Core.Services.Archives.ArchiveCache>();
        services.AddSingleton<Services.ArchivePasswordStore>();
        services.AddSingleton<BertBrowser.Core.Services.Archives.IArchivePasswords>(
            s => s.GetRequiredService<Services.ArchivePasswordStore>());
        services.AddSingleton<BertBrowser.Core.Services.Archives.ArchiveAwareFileSystemService>(
            s => new BertBrowser.Core.Services.Archives.ArchiveAwareFileSystemService(
                s.GetRequiredService<FileSystemService>(),
                s.GetRequiredService<BertBrowser.Core.Services.Archives.IArchiveReader>(),
                s.GetRequiredService<BertBrowser.Core.Services.Archives.IArchivePasswords>(),
                s.GetRequiredService<BertBrowser.Core.Services.Archives.ArchiveCache>()));
        services.AddSingleton<IFileSystemService>(
            s => s.GetRequiredService<BertBrowser.Core.Services.Archives.ArchiveAwareFileSystemService>());
        // Same object again under its other interface: the listing seam for everything that browses
        // directories, and this one for the few places that must be able to tell an archive apart.
        // One instance, so the two views can never disagree about what a path is.
        services.AddSingleton<BertBrowser.Core.Services.Archives.IArchiveBrowser>(
            s => s.GetRequiredService<BertBrowser.Core.Services.Archives.ArchiveAwareFileSystemService>());
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferPlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferExecutor>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenamePlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenameExecutor>();
        services.AddSingleton<BertBrowser.Core.Services.Archives.ArchiveCreator>();
        services.AddSingleton<BertBrowser.Core.Services.Archives.ArchiveEditPlanner>();
        services.AddSingleton(s => new BertBrowser.Core.Services.Archives.ArchiveEditExecutor(
            s.GetRequiredService<BertBrowser.Core.Services.Archives.IArchiveReader>()));
        services.AddSingleton<BertBrowser.Core.Services.Archives.ExtractPlanner>();
        services.AddSingleton(s => new BertBrowser.Core.Services.Archives.ExtractExecutor(
            s.GetRequiredService<BertBrowser.Core.Services.Archives.IArchiveReader>()));
        services.AddSingleton<BertBrowser.Core.Services.NewItem.NewItemPlanner>();
        services.AddSingleton<BertBrowser.Core.Services.NewItem.NewItemExecutor>();
        services.AddSingleton<IShellNewCatalog, ShellNewCatalog>();
        services.AddSingleton<IFolderHandlerService, FolderHandlerService>();
        // One instance serving both roles: it caches per-volume answers, and the planner and the
        // executor should agree about what has a Recycle Bin.
        services.AddSingleton<BertBrowser.Core.Services.Delete.ShellRecycleBin>();
        services.AddSingleton<BertBrowser.Core.Services.Delete.IRecycleBin>(
            s => s.GetRequiredService<BertBrowser.Core.Services.Delete.ShellRecycleBin>());
        services.AddSingleton<BertBrowser.Core.Services.Delete.IRecycleProbe>(
            s => s.GetRequiredService<BertBrowser.Core.Services.Delete.ShellRecycleBin>());
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
        // Not MftIndexService: reading the MFT needs an administrator token, and this process
        // deliberately does not have one. The client starts BertBrowser.Indexer.exe elevated and
        // mirrors what it reports, so everything above this line is unchanged by the split.
        services.AddSingleton<IIndexHostLauncher, ElevatedIndexHostLauncher>();
        services.AddSingleton<IIndexTransportFactory, NamedPipeIndexTransportFactory>();
        services.AddSingleton<IMftIndexService, MftIndexClient>();
        // The other elevated helper: one short-lived process per file operation Windows refused,
        // started only from a click on a shield. Nothing here runs at launch, and nothing retries on
        // a timer — every attempt is a UAC prompt.
        services.AddSingleton<BertBrowser.Core.Services.Elevation.IElevationLauncher,
            BertBrowser.App.Services.Elevation.ElevatedFileOperationLauncher>();
        services.AddSingleton<BertBrowser.Core.Services.Elevation.IElevationTransportFactory,
            BertBrowser.App.Services.Elevation.NamedPipeElevationTransportFactory>();
        services.AddSingleton<BertBrowser.Core.Services.Elevation.IElevationPrompt,
            Views.ElevationPrompt>();
        services.AddSingleton<BertBrowser.Core.Services.Elevation.IElevatedOperationRunner>(
            s => new BertBrowser.Core.Services.Elevation.ElevationClient(
                s.GetRequiredService<BertBrowser.Core.Services.Elevation.IElevationLauncher>(),
                s.GetRequiredService<BertBrowser.Core.Services.Elevation.IElevationTransportFactory>(),
                BertBrowser.App.Services.Elevation.ElevatedProcess.CurrentUserSid ?? ""));
        // The one content-search bound a person can move. A delegate, so the setting can change
        // while this singleton lives, and Core still never sees AppSettings.
        services.AddSingleton<ISearchService>(sp => new SearchService(
            sp.GetRequiredService<FsIndexRepository>(),
            sp.GetRequiredService<IndexCrawler>(),
            sp.GetRequiredService<IIndexWatcherService>(),
            sp.GetRequiredService<BertBrowser.Core.Services.Mft.IMftIndexService>(),
            sp.GetRequiredService<BertBrowser.Core.Services.Archives.IArchiveBrowser>(),
            contentReader: null,
            contentBudget: () => sp.GetRequiredService<AppSettings>().SearchContentMaxBytes));
        services.AddSingleton<BertBrowser.Core.Services.DiskUsage.IDiskUsageService,
            BertBrowser.Core.Services.DiskUsage.DiskUsageService>();
        // Finding duplicates starts from the byte lengths the MFT pass already wrote, then reads
        // only the files that collide — so the shortlist, the hasher and the facade over both are
        // three registrations rather than one service that does its own I/O.
        services.AddSingleton<BertBrowser.Core.Services.Duplicates.IDuplicateCandidateSource,
            BertBrowser.Core.Services.Duplicates.IndexedDuplicateCandidateSource>();
        services.AddSingleton<BertBrowser.Core.Services.Duplicates.IFileHasher,
            BertBrowser.Core.Services.Duplicates.FileSystemFileHasher>();
        services.AddSingleton<BertBrowser.Core.Services.Duplicates.IDuplicateFinder,
            BertBrowser.Core.Services.Duplicates.DuplicateFinder>();
        // Comparing two folders reads each side from whichever source can answer it — the index
        // when the volume is measured and live, a walk otherwise — so it needs both the repository
        // and the MFT service, and no I/O of its own.
        services.AddSingleton<BertBrowser.Core.Services.Compare.IFolderCompareService>(sp =>
            new BertBrowser.Core.Services.Compare.FolderCompareService(
                sp.GetRequiredService<FsIndexRepository>(),
                sp.GetRequiredService<BertBrowser.Core.Services.Mft.IMftIndexService>()));
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
            // Restore and raise: the request is worthless if the window it opens in stays behind
            // whatever the user was actually looking at. Activate() alone is not enough from a
            // background process — see ForegroundWindow, and note the sending copy has already
            // handed over its right to do this.
            ForegroundWindow.Raise(window);

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
