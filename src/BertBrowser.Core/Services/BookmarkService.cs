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

    public Task<bool> AddAsync(string path, bool isDirectory) =>
        Task.Run(() => _repository.Add(path, isDirectory));

    public Task RemoveAsync(string path) =>
        Task.Run(() => _repository.Remove(path));

    public Task<bool> ExistsAsync(string path) =>
        Task.Run(() => _repository.Exists(path));
}
