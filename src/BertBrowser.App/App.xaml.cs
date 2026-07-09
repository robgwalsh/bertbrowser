using System.Windows;
using BertBrowser.App.Services;
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
        services.AddSingleton(new Db(AppPaths.DbPath));
        services.AddSingleton<TagRepository>();
        services.AddSingleton<DirSizeRepository>();
        services.AddSingleton<FsIndexRepository>();
        services.AddSingleton<BookmarkRepository>();
        services.AddSingleton<IFileSystemService, FileSystemService>();
        services.AddSingleton<IDirectorySizeService, DirectorySizeService>();
        services.AddSingleton<IFileTransferService, FileTransferService>();
        services.AddSingleton<ITagService, TagService>();
        services.AddSingleton<IBookmarkService, BookmarkService>();
        services.AddSingleton<IndexCrawler>();
        services.AddSingleton<IIndexWatcherService, IndexWatcherService>();
        services.AddSingleton<IMftIndexService, MftIndexService>();
        services.AddSingleton<ISearchService, SearchService>();
        services.AddSingleton<IUpdateService, UpdateService>();
        services.AddSingleton<ShellViewModel>();
        services.AddSingleton<MainWindow>();
        Services = services.BuildServiceProvider();

        Services.GetRequiredService<Db>().Migrate();

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
    }

    protected override void OnExit(ExitEventArgs e)
    {
        // Disposes IDisposable singletons (index watchers, search service).
        (Services as IDisposable)?.Dispose();
        base.OnExit(e);
    }
}
