using BertBrowser.Core.Data;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services.SavedWorkspaces;

public interface ISavedWorkspaceService
{
    Task<IReadOnlyList<SavedWorkspace>> GetAllAsync();
    Task SaveAsync(SavedWorkspace workspace);
    Task<bool> RenameAsync(string oldName, string newName);
    Task RemoveAsync(string name);
}

/// <summary>Async facade over <see cref="SavedWorkspaceRepository"/> so view models never block
/// the UI thread on SQLite. Validation is not here: the rules run in the dialog, where a refusal
/// can be worded, before anything reaches this.</summary>
public sealed class SavedWorkspaceService : ISavedWorkspaceService
{
    private readonly SavedWorkspaceRepository _repository;

    public SavedWorkspaceService(SavedWorkspaceRepository repository) => _repository = repository;

    public Task<IReadOnlyList<SavedWorkspace>> GetAllAsync() =>
        Task.Run(() => _repository.GetAll());

    public Task SaveAsync(SavedWorkspace workspace) =>
        Task.Run(() => _repository.Save(workspace));

    public Task<bool> RenameAsync(string oldName, string newName) =>
        Task.Run(() => _repository.Rename(oldName, newName));

    public Task RemoveAsync(string name) =>
        Task.Run(() => _repository.Remove(name));
}
