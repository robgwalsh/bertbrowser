using BertBrowser.Core.Data;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Services;

public interface ITagService
{
    Task<IReadOnlyList<Tag>> GetAllTagsAsync();
    Task<Tag> CreateTagAsync(string name, string? color);
    Task RenameTagAsync(long tagId, string newName);
    Task SetTagColorAsync(long tagId, string? color);
    Task DeleteTagAsync(long tagId);
    Task<int> GetTagUsageCountAsync(long tagId);

    Task AssignTagsAsync(IReadOnlyCollection<string> paths, IReadOnlyCollection<long> tagIds);
    Task UnassignTagsAsync(IReadOnlyCollection<string> paths, IReadOnlyCollection<long> tagIds);
    Task RemoveFileAsync(string path);

    Task<IReadOnlyDictionary<string, IReadOnlyList<Tag>>> GetTagsForPathsAsync(IReadOnlyCollection<string> paths);
    Task<IReadOnlyList<TaggedFile>> QueryTaggedFilesUnderAsync(string directory, IReadOnlyCollection<long> tagIds, TagMatchMode mode);
    Task<IReadOnlyDictionary<long, int>> GetTagCountsUnderAsync(string directory);
}

/// <summary>Async facade over TagRepository so ViewModels never block the UI thread on SQLite.</summary>
public sealed class TagService : ITagService
{
    private readonly TagRepository _repository;

    public TagService(TagRepository repository) => _repository = repository;

    public Task<IReadOnlyList<Tag>> GetAllTagsAsync() =>
        Task.Run(() => _repository.GetAllTags());

    public Task<Tag> CreateTagAsync(string name, string? color) =>
        Task.Run(() => _repository.CreateTag(name, color));

    public Task RenameTagAsync(long tagId, string newName) =>
        Task.Run(() => _repository.RenameTag(tagId, newName));

    public Task SetTagColorAsync(long tagId, string? color) =>
        Task.Run(() => _repository.SetTagColor(tagId, color));

    public Task DeleteTagAsync(long tagId) =>
        Task.Run(() => _repository.DeleteTag(tagId));

    public Task<int> GetTagUsageCountAsync(long tagId) =>
        Task.Run(() => _repository.GetTagUsageCount(tagId));

    public Task AssignTagsAsync(IReadOnlyCollection<string> paths, IReadOnlyCollection<long> tagIds) =>
        Task.Run(() => _repository.AssignTags(paths, tagIds));

    public Task UnassignTagsAsync(IReadOnlyCollection<string> paths, IReadOnlyCollection<long> tagIds) =>
        Task.Run(() => _repository.UnassignTags(paths, tagIds));

    public Task RemoveFileAsync(string path) =>
        Task.Run(() => _repository.RemoveFile(path));

    public Task<IReadOnlyDictionary<string, IReadOnlyList<Tag>>> GetTagsForPathsAsync(IReadOnlyCollection<string> paths) =>
        Task.Run(() => _repository.GetTagsForPaths(paths));

    public Task<IReadOnlyList<TaggedFile>> QueryTaggedFilesUnderAsync(
        string directory, IReadOnlyCollection<long> tagIds, TagMatchMode mode) =>
        Task.Run(() => _repository.QueryTaggedFilesUnder(directory, tagIds, mode));

    public Task<IReadOnlyDictionary<long, int>> GetTagCountsUnderAsync(string directory) =>
        Task.Run(() => _repository.GetTagCountsUnder(directory));
}
