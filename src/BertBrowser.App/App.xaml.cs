using System.Windows;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using BertBrowser.App.Views;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App;

public partial class App : Application
{
    public static IServiceProvider Services { get; private set; } = null!;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        var dbPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "BertBrowser", "bertbrowser.db");

        var services = new ServiceCollection();
        services.AddSingleton(AppSettings.Load());
        services.AddSingleton(new Db(dbPath));
        services.AddSingleton<TagRepository>();
        services.AddSingleton<DirSizeRepository>();
        services.AddSingleton<FsIndexRepository>();
        services.AddSingleton<IFileSystemService, FileSystemService>();
        services.AddSingleton<IDirectorySizeService, DirectorySizeService>();
        services.AddSingleton<ITagService, TagService>();
        services.AddSingleton<IndexCrawler>();
        services.AddSingleton<IIndexWatcherService, IndexWatcherService>();
        services.AddSingleton<ISearchService, SearchService>();
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
    }

    protected override void OnExit(ExitEventArgs e)
    {
        // Disposes IDisposable singletons (index watchers, search service).
        (Services as IDisposable)?.Dispose();
        base.OnExit(e);
    }
}
