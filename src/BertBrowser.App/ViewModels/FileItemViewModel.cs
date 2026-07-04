using System.Windows.Media;
using BertBrowser.App.Interop;
using CommunityToolkit.Mvvm.ComponentModel;
using BertBrowser.Core.Models;
using BertBrowser.Core.Services;

namespace BertBrowser.App.ViewModels;

public sealed partial class FileItemViewModel : ObservableObject
{
    public string Name { get; }
    public string FullPath { get; }
    public bool IsDirectory { get; }
    public DateTime ModifiedUtc { get; private set; }
    public string TypeName { get; }

    /// <summary>Path relative to the filter root; only set in flattened tag-filter mode.</summary>
    public string RelativePath { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay), nameof(SizeSortKey))]
    private long? _sizeBytes;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SizeDisplay))]
    private bool _sizeIncomplete;

    [ObservableProperty]
    private bool _isSizeComputing;

    [ObservableProperty]
    private DateTime? _sizeComputedUtc;

    [ObservableProperty]
    private bool _isMissing;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(TagsDisplay), nameof(TagsSortKey))]
    private IReadOnlyList<Tag> _tags = Array.Empty<Tag>();

    private ImageSource? _icon;
    private bool _iconLoaded;

    public FileItemViewModel(FileEntry entry)
    {
        Name = entry.Name;
        FullPath = entry.FullPath;
        IsDirectory = entry.IsDirectory;
        ModifiedUtc = entry.ModifiedUtc;
        RelativePath = string.Empty;
        SizeBytes = entry.IsDirectory ? null : entry.SizeBytes;
        TypeName = entry.IsDirectory
            ? "Folder"
            : Path.GetExtension(entry.Name) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    /// <summary>Search-result mode: a real filesystem entry (file or directory) plus its
    /// parent path relative to the search root.</summary>
    public FileItemViewModel(FileEntry entry, string relativePath) : this(entry)
    {
        RelativePath = relativePath;
    }

    public FileItemViewModel(string fullPath, string relativePath, IReadOnlyList<Tag> tags)
    {
        Name = Path.GetFileName(fullPath);
        FullPath = fullPath;
        IsDirectory = false;
        RelativePath = relativePath;
        Tags = tags;
        TypeName = Path.GetExtension(fullPath) is { Length: > 1 } ext ? ext[1..].ToUpperInvariant() + " file" : "File";
    }

    /// <summary>Fills size/modified from disk in flattened mode; marks missing files.</summary>
    public void HydrateFromDisk()
    {
        var info = new FileInfo(FullPath);
        if (info.Exists)
        {
            SizeBytes = info.Length;
            ModifiedUtc = info.LastWriteTimeUtc;
            OnPropertyChanged(nameof(ModifiedDisplay));
        }
        else
        {
            IsMissing = true;
        }
    }

    public ImageSource? Icon
    {
        get
        {
            if (!_iconLoaded)
            {
                _iconLoaded = true;
                _icon = ShellIcons.GetIcon(FullPath, IsDirectory);
            }
            return _icon;
        }
    }

    public string SizeDisplay =>
        SizeBytes is { } b
            ? ByteSizeFormatter.Format(b) + (SizeIncomplete ? " *" : "")
            : IsDirectory ? "—" : "";

    public string ModifiedDisplay => ModifiedUtc == default ? "" : ModifiedUtc.ToLocalTime().ToString("g");

    public string TagsDisplay => string.Join(", ", Tags.Select(t => t.Name));

    /// <summary>Unknown sizes sort together at the small end.</summary>
    public long SizeSortKey => SizeBytes ?? -1;

    public string TagsSortKey =>
        Tags.Count == 0
            ? "￿" // untagged last
            : string.Join(",", Tags.Select(t => t.Name).OrderBy(n => n, StringComparer.OrdinalIgnoreCase));

    public string SizeTooltip =>
        IsDirectory && SizeComputedUtc is { } computed
            ? $"Computed {computed.ToLocalTime():g}" + (SizeIncomplete ? "\nMay be incomplete — some folders were inaccessible." : "")
            : SizeDisplay;
}
