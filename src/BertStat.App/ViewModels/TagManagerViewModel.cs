using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using BertStat.Core.Models;
using BertStat.Core.Services;

namespace BertStat.App.ViewModels;

public sealed partial class TagManagerViewModel : ObservableObject
{
    private readonly ITagService _tagService;

    public ObservableCollection<Tag> Tags { get; } = new();

    [ObservableProperty]
    private Tag? _selectedTag;

    [ObservableProperty]
    private string _newTagName = "";

    [ObservableProperty]
    private string _newTagColor = "#607D8B";

    [ObservableProperty]
    private string? _errorMessage;

    /// <summary>Set by the window to confirm destructive deletes; returns true to proceed.</summary>
    public Func<string, bool>? ConfirmAction { get; set; }

    public TagManagerViewModel(ITagService tagService) => _tagService = tagService;

    public async Task LoadAsync()
    {
        Tags.Clear();
        foreach (var tag in await _tagService.GetAllTagsAsync())
            Tags.Add(tag);
    }

    [RelayCommand]
    private async Task AddTagAsync()
    {
        ErrorMessage = null;
        var name = NewTagName.Trim();
        if (name.Length == 0) return;

        try
        {
            await _tagService.CreateTagAsync(name, NormalizeColor(NewTagColor));
            NewTagName = "";
            await LoadAsync();
        }
        catch (Microsoft.Data.Sqlite.SqliteException)
        {
            ErrorMessage = $"A tag named \"{name}\" already exists.";
        }
    }

    [RelayCommand]
    private async Task RenameTagAsync((Tag Tag, string NewName) args)
    {
        var name = args.NewName.Trim();
        if (name.Length == 0 || name == args.Tag.Name) return;

        try
        {
            await _tagService.RenameTagAsync(args.Tag.Id, name);
            await LoadAsync();
        }
        catch (Microsoft.Data.Sqlite.SqliteException)
        {
            ErrorMessage = $"A tag named \"{name}\" already exists.";
        }
    }

    [RelayCommand]
    private async Task SetColorAsync((Tag Tag, string Color) args)
    {
        var color = NormalizeColor(args.Color);
        if (color is null) return;
        await _tagService.SetTagColorAsync(args.Tag.Id, color);
        await LoadAsync();
    }

    [RelayCommand]
    private async Task DeleteTagAsync(Tag? tag)
    {
        if (tag is null) return;

        var usage = await _tagService.GetTagUsageCountAsync(tag.Id);
        var message = usage > 0
            ? $"Delete tag \"{tag.Name}\"? It is assigned to {usage} file(s); those assignments will be removed."
            : $"Delete tag \"{tag.Name}\"?";
        if (ConfirmAction?.Invoke(message) != true) return;

        await _tagService.DeleteTagAsync(tag.Id);
        await LoadAsync();
    }

    private static string? NormalizeColor(string input)
    {
        var s = input.Trim();
        if (s.Length == 0) return null;
        if (!s.StartsWith('#')) s = "#" + s;
        return System.Text.RegularExpressions.Regex.IsMatch(s, "^#[0-9a-fA-F]{6}$") ? s.ToUpperInvariant() : null;
    }
}
