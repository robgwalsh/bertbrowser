using BertBrowser.App.Services;
using BertBrowser.Core.Data;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// Creates tabs (and later, panes). Tabs are made at runtime with a start path, which a plain DI
/// registration can't express, and the alternative — handing the view models an
/// <see cref="IServiceProvider"/> — would put a service locator in the VM layer. A small factory
/// keeps the dependency list visible in one place and matches how the shell already hand-builds
/// its children.
/// </summary>
/// <remarks>
/// The returned tab is not navigated anywhere: the caller decides whether to await the first load
/// (startup) or let it run in the background (a new tab opened while the user keeps working).
/// </remarks>
public sealed class PaneFactory(
    IFileSystemService fileSystem,
    DirSizeRepository dirSizeRepository,
    ISearchService searchService,
    AppSettings settings,
    IProcessLauncher launcher,
    BertBrowser.Core.Services.Mft.IMftIndexService mftIndex)
{
    public DirectoryTabViewModel CreateTab() =>
        new(fileSystem, dirSizeRepository, searchService, settings, launcher, mftIndex);

    /// <summary>A tab set up to look like <paramref name="source"/>: same sort order. History is
    /// deliberately not copied — the duplicate starts fresh.</summary>
    public DirectoryTabViewModel CloneTab(DirectoryTabViewModel source)
    {
        var tab = CreateTab();
        tab.FileList.SortBy = source.FileList.SortBy;
        tab.FileList.SortDescending = source.FileList.SortDescending;
        return tab;
    }
}
