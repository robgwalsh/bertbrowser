using BertBrowser.App.Views;
using System.Windows;
using Velopack;
using Velopack.Sources;

namespace BertBrowser.App.Services;

public interface IUpdateService
{
    /// <summary>Check GitHub for a newer release, download it, and stage it to apply on exit.</summary>
    Task CheckAndStageUpdateAsync();
}

public sealed class UpdateService : IUpdateService
{
    private const string RepoUrl = "https://github.com/robgwalsh/bertbrowser";

    private readonly UpdateManager _manager;

    public UpdateService()
    {
        // BERTBROWSER_UPDATE_URL points at a local Releases directory (or any static
        // file host) for end-to-end update testing without touching GitHub.
        var overrideUrl = Environment.GetEnvironmentVariable("BERTBROWSER_UPDATE_URL");

        // Which GitHub releases to look at; Velopack then picks this build's own channel out of
        // them (releases.win.json for a release, releases.unstable.json for a build off main).
        // Both arms matter. An unstable build ships as a pre-release, so refusing pre-releases
        // would leave it unable to ever see its own feed; a release must keep refusing them, or the
        // newest thing on offer is the unstable pre-release, which carries no feed for its channel.
        var prerelease = AppVersion.IsUnstable;

        _manager = overrideUrl is { Length: > 0 }
            ? new UpdateManager(overrideUrl)
            : new UpdateManager(new GithubSource(RepoUrl, accessToken: null, prerelease));
    }

    public async Task CheckAndStageUpdateAsync()
    {
        // Dev builds (dotnet run / F5) are not Velopack-installed; updating is meaningless there.
        if (!_manager.IsInstalled)
            return;

        try
        {
            var update = await _manager.CheckForUpdatesAsync();
            if (update is null)
                return;

            await _manager.DownloadUpdatesAsync(update);

            // Updates are mandatory: stage the update to apply when the process exits,
            // whether or not the user restarts now.
            _manager.WaitExitThenApplyUpdates(update, silent: true, restart: false);

            var version = update.TargetFullRelease.Version;
            var restartNow = await Application.Current.Dispatcher.InvokeAsync(() =>
                MessageDialog.Show(
                    Application.Current.MainWindow,
                    $"BertBrowser {version} has been downloaded and will be installed when you close the app.\n\nRestart now to update immediately?",
                    "Update ready",
                    MessageDialogKind.Information,
                    showCancel: true));

            if (restartNow)
                _manager.ApplyUpdatesAndRestart(update);
        }
        catch (Exception)
        {
            // Never let a failed update check take down the app; next launch retries.
        }
    }
}
