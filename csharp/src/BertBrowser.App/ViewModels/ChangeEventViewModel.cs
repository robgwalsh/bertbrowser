using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Changes;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.ViewModels;

/// <summary>One row of the "What changed" list: a recorded change, worded for a person.</summary>
public sealed partial class ChangeEventViewModel : ObservableObject
{
    private readonly ChangeRow _row;

    public ChangeEventViewModel(ChangeRow row, DateTime nowUtc)
    {
        _row = row;
        _whenText = RelativeTime.Format(row.LastUtc, nowUtc);
    }

    public ChangeKind Kind => _row.Kind;

    public string KindLabel => Kind switch
    {
        ChangeKind.Created => "Created",
        ChangeKind.Modified => "Modified",
        ChangeKind.Deleted => "Deleted",
        ChangeKind.Renamed => "Renamed",
        _ => Kind.ToString(),
    };

    public string FullPath => _row.DisplayPath;

    public string Name =>
        Path.GetFileName(_row.DisplayPath) is { Length: > 0 } name ? name : _row.DisplayPath;

    public string Folder => Path.GetDirectoryName(_row.DisplayPath) ?? "";

    public bool IsDirectory => _row.IsDirectory;

    public bool IsDeleted => Kind == ChangeKind.Deleted;

    /// <summary>"was setup.part", for a rename; empty otherwise. The old <em>name</em> — a rename
    /// within one folder is the common case, and the folder is already on the row.</summary>
    public string RenamedFrom =>
        Kind == ChangeKind.Renamed && _row.OldDisplayPath is { } old && Path.GetFileName(old) is { Length: > 0 } name
            ? $"was {name}"
            : "";

    public bool HasRenamedFrom => RenamedFrom.Length > 0;

    /// <summary>"×412" when the row stands for a burst of writes; empty for one.</summary>
    public string CountText => _row.Count > 1 ? $"×{_row.Count:N0}" : "";

    public DateTime LastUtc => _row.LastUtc;

    public string WhenTooltip => _row.Count > 1
        ? $"First {_row.FirstUtc.ToLocalTime():g}, last {_row.LastUtc.ToLocalTime():g} — {_row.Count:N0} times"
        : _row.LastUtc.ToLocalTime().ToString("g");

    /// <summary>"3 min ago" — re-aged by <see cref="Touch"/> while the window stays open.</summary>
    [ObservableProperty]
    private string _whenText;

    public void Touch(DateTime nowUtc) => WhenText = RelativeTime.Format(_row.LastUtc, nowUtc);

    /// <summary>One tab-separated line for the clipboard; pastes into a spreadsheet as columns.</summary>
    public string CopyLine
    {
        get
        {
            var line = $"{KindLabel}\t{FullPath}\t{_row.LastUtc.ToLocalTime():g}";
            if (_row.Count > 1) line += $"\t×{_row.Count}";
            if (Kind == ChangeKind.Renamed && _row.OldDisplayPath is { } old) line += $"\tfrom {old}";
            return line;
        }
    }
}
