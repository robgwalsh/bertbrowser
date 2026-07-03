using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertStat.Core.Models;
using BertStat.Core.Paths;
using BertStat.Core.Services;

namespace BertStat.App.ViewModels;

public sealed partial class AssignTagItemViewModel : ObservableObject
{
    public Tag Tag { get; }

    /// <summary>null = indeterminate (assigned to some of the selected files).</summary>
    [ObservableProperty]
    private bool? _isChecked;

    public bool? OriginalState { get; }

    public AssignTagItemViewModel(Tag tag, bool? initialState)
    {
        Tag = tag;
        OriginalState = initialState;
        IsChecked = initialState;
    }
}

public sealed partial class AssignTagsViewModel : ObservableObject
{
    private readonly ITagService _tagService;
    private readonly IReadOnlyList<string> _paths;

    public ObservableCollection<AssignTagItemViewModel> Tags { get; } = new();

    [ObservableProperty]
    private string _newTagName = "";

    public string Title => _paths.Count == 1
        ? $"Tags for {Path.GetFileName(_paths[0])}"
        : $"Tags for {_paths.Count} files";

    public AssignTagsViewModel(ITagService tagService, IReadOnlyList<string> paths)
    {
        _tagService = tagService;
        _paths = paths;
    }

    public async Task LoadAsync()
    {
        var allTags = await _tagService.GetAllTagsAsync();
        var existing = await _tagService.GetTagsForPathsAsync(_paths);

        var perTagCount = new Dictionary<long, int>();
        foreach (var path in _paths)
        {
            if (!existing.TryGetValue(PathKey.Canonicalize(path), out var tags)) continue;
            foreach (var tag in tags)
                perTagCount[tag.Id] = perTagCount.TryGetValue(tag.Id, out var c) ? c + 1 : 1;
        }

        Tags.Clear();
        foreach (var tag in allTags)
        {
            bool? state = perTagCount.TryGetValue(tag.Id, out var count)
                ? count == _paths.Count ? true : null
                : false;
            Tags.Add(new AssignTagItemViewModel(tag, state));
        }
    }

    [RelayCommand]
    private async Task CreateTagAsync()
    {
        var name = NewTagName.Trim();
        if (name.Length == 0) return;

        try
        {
            var tag = await _tagService.CreateTagAsync(name, null);
            Tags.Add(new AssignTagItemViewModel(tag, false) { IsChecked = true });
            NewTagName = "";
        }
        catch (Microsoft.Data.Sqlite.SqliteException)
        {
            // duplicate name: check the existing entry instead
            var match = Tags.FirstOrDefault(t => t.Tag.Name.Equals(name, StringComparison.OrdinalIgnoreCase));
            if (match is not null) match.IsChecked = true;
        }
    }

    /// <summary>Applies check-state changes: checked → assign to all, unchecked → unassign from all.</summary>
    public async Task ApplyAsync()
    {
        var toAssign = Tags.Where(t => t.IsChecked == true && t.OriginalState != true)
            .Select(t => t.Tag.Id).ToList();
        var toUnassign = Tags.Where(t => t.IsChecked == false && t.OriginalState != false)
            .Select(t => t.Tag.Id).ToList();

        if (toAssign.Count > 0)
            await _tagService.AssignTagsAsync(_paths, toAssign);
        if (toUnassign.Count > 0)
            await _tagService.UnassignTagsAsync(_paths, toUnassign);
    }
}
