using BertBrowser.Core.Data;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services;

public interface IBookmarkService
{
    Task<IReadOnlyList<Bookmark>> GetAllAsync();
    Task<bool> AddAsync(string path, bool isDirectory);
    Task RemoveAsync(string path);
    Task<bool> ExistsAsync(string path);
}

/// <summary>Async facade over BookmarkRepository so ViewModels never block the UI thread on SQLite.</summary>
public sealed class BookmarkService : IBookmarkService
{
    private readonly BookmarkRepository _repository;

    public BookmarkService(BookmarkRepository repository) => _repository = repository;

    public Task<IReadOnlyList<Bookmark>> GetAllAsync() =>
        Task.Run(() => _repository.GetAll());

    /// <remarks>
    /// <b>A path inside an archive is refused here, not only in the menu.</b> The bookmark table is
    /// keyed by <c>PathKey</c>, and a virtual path canonicalizes perfectly happily — which is the
    /// trap: <c>PathKey.IsUnder</c> then places it strictly inside the archive's own containing
    /// folder as well as inside the archive, so one such row would make every subtree range scan
    /// over that folder start returning archive interiors. The menu hiding the item is a courtesy;
    /// this is the rule.
    /// </remarks>
    public Task<bool> AddAsync(string path, bool isDirectory)
    {
        if (Archives.ArchivePath.Parse(path, File.Exists) is not null) return Task.FromResult(false);
        return Task.Run(() => _repository.Add(path, isDirectory));
    }

    public Task RemoveAsync(string path) =>
        Task.Run(() => _repository.Remove(path));

    public Task<bool> ExistsAsync(string path) =>
        Task.Run(() => _repository.Exists(path));
}
