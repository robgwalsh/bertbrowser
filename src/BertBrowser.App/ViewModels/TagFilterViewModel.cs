using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed partial class TagFilterItemViewModel : ObservableObject
{
    private readonly TagFilterViewModel _owner;

    public Tag Tag { get; }

    [ObservableProperty]
    private bool _isChecked;

    [ObservableProperty]
    private int _count;

    public TagFilterItemViewModel(TagFilterViewModel owner, Tag tag, int count)
    {
        _owner = owner;
        Tag = tag;
        Count = count;
    }

    partial void OnIsCheckedChanged(bool value) => _owner.RaiseFilterChanged();
}

public sealed partial class TagFilterViewModel : ObservableObject
{
    private readonly ITagService _tagService;
    private bool _suppressEvents;

    public ObservableCollection<TagFilterItemViewModel> Tags { get; } = new();

    [ObservableProperty]
    private bool _matchAll;

    public event Action? FilterChanged;

    public bool IsActive => Tags.Any(t => t.IsChecked);

    public IReadOnlyList<long> CheckedTagIds =>
        Tags.Where(t => t.IsChecked).Select(t => t.Tag.Id).ToList();

    public TagFilterViewModel(ITagService tagService) => _tagService = tagService;

    partial void OnMatchAllChanged(bool value)
    {
        if (IsActive) RaiseFilterChanged();
    }

    [RelayCommand]
    private void Clear()
    {
        _suppressEvents = true;
        foreach (var tag in Tags)
            tag.IsChecked = false;
        _suppressEvents = false;
        RaiseFilterChanged();
    }

    internal void RaiseFilterChanged()
    {
        if (!_suppressEvents)
            FilterChanged?.Invoke();
    }

    /// <summary>Reloads the tag list and per-tag counts for the current directory, keeping check states.</summary>
    public async Task RefreshAsync(string currentDirectory)
    {
        var allTags = await _tagService.GetAllTagsAsync();
        var counts = await _tagService.GetTagCountsUnderAsync(currentDirectory);
        var checkedIds = Tags.Where(t => t.IsChecked).Select(t => t.Tag.Id).ToHashSet();

        _suppressEvents = true;
        Tags.Clear();
        foreach (var tag in allTags)
        {
            var item = new TagFilterItemViewModel(this, tag, counts.TryGetValue(tag.Id, out var c) ? c : 0)
            {
                IsChecked = checkedIds.Contains(tag.Id),
            };
            Tags.Add(item);
        }
        _suppressEvents = false;

        // A checked tag may have been deleted; if the effective filter changed, re-raise.
        if (!checkedIds.SetEquals(Tags.Where(t => t.IsChecked).Select(t => t.Tag.Id)))
            RaiseFilterChanged();
    }
}
