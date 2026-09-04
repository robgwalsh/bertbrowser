using BertBrowser.Core.Data;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.SavedSearches;

public interface ISavedSearchService
{
    Task<IReadOnlyList<SavedSearch>> GetAllAsync();
    Task SaveAsync(SavedSearch search);
    Task<bool> RenameAsync(string oldName, string newName);
    Task RemoveAsync(string name);
}

/// <summary>Async facade over <see cref="SavedSearchRepository"/> so view models never block the
/// UI thread on SQLite. Validation is not here: the rules run in the dialog, where a refusal can
/// be worded, before anything reaches this.</summary>
public sealed class SavedSearchService : ISavedSearchService
{
    private readonly SavedSearchRepository _repository;

    public SavedSearchService(SavedSearchRepository repository) => _repository = repository;

    public Task<IReadOnlyList<SavedSearch>> GetAllAsync() =>
        Task.Run(() => _repository.GetAll());

    public Task SaveAsync(SavedSearch search) =>
        Task.Run(() => _repository.Save(search));

    public Task<bool> RenameAsync(string oldName, string newName) =>
        Task.Run(() => _repository.Rename(oldName, newName));

    public Task RemoveAsync(string name) =>
        Task.Run(() => _repository.Remove(name));
}
