using System.Windows;
using BertBrowser.App.Services;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Mft;
using Microsoft.Extensions.DependencyInjection;
using Velopack;

namespace BertBrowser.App;

public partial class App : Application
{
    public static IServiceProvider Services { get; private set; } = null!;

    [STAThread]
    private static void Main(string[] args)
    {
        // Must run before any WPF code: handles Velopack install/update/uninstall
        // hooks and exits the process when invoked as one.
        VelopackApp.Build().Run();

        var app = new App();
        app.InitializeComponent();
        app.Run();
    }

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        AppPaths.MigrateLegacyData();

        var services = new ServiceCollection();
        services.AddSingleton(AppSettings.Load());
        services.AddSingleton<UserThemeStore>();
        services.AddSingleton<IThemeService, ThemeService>();
        services.AddSingleton(new Db(AppPaths.DbPath));
        services.AddSingleton<DirSizeRepository>();
        services.AddSingleton<FsIndexRepository>();
        services.AddSingleton<BookmarkRepository>();
        services.AddSingleton<IFileSystemService, FileSystemService>();
        services.AddSingleton<IDirectorySizeService, DirectorySizeService>();
        services.AddSingleton<IFileTransferService, FileTransferService>();
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferPlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Transfer.TransferExecutor>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenamePlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Rename.RenameExecutor>();
        services.AddSingleton<BertBrowser.Core.Services.Delete.DeletePlanner>();
        services.AddSingleton<BertBrowser.Core.Services.Delete.DeleteExecutor>();
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
        Services = services.BuildServiceProvider();

        Services.GetRequiredService<Db>().Migrate();

        // Before any window exists, so the first frame is already in the chosen theme.
        Services.GetRequiredService<IThemeService>().Initialize();

        // Start path priority: command-line argument, then last visited, then user profile.
        var settings = Services.GetRequiredService<AppSettings>();
        var shell = Services.GetRequiredService<ShellViewModel>();
        if (e.Args.Length > 0 && Directory.Exists(e.Args[0]))
            shell.StartPath = e.Args[0];
        else if (settings.LastPath is { } last && Directory.Exists(last))
            shell.StartPath = last;

        var window = Services.GetRequiredService<MainWindow>();
        window.Show();

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

    protected override void OnExit(ExitEventArgs e)
    {
        // Disposes IDisposable singletons (index watchers, search service).
        (Services as IDisposable)?.Dispose();
        base.OnExit(e);
    }
}
