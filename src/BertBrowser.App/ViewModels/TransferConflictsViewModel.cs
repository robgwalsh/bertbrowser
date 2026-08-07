using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.ViewModels;

/// <summary>One name that already exists at the destination, with both sides described so the
/// user can tell which copy is which before deciding.</summary>
public sealed class TransferConflictViewModel
{
    public string Name { get; }
    public string IncomingDetail { get; }
    public string ExistingDetail { get; }

    /// <summary>Highlighted in the dialog: the common case for wanting Replace.</summary>
    public bool IncomingIsNewer { get; }

    public TransferConflictViewModel(PlannedTransfer transfer)
    {
        Name = transfer.Name;
        var incoming = Describe(transfer.SourcePath, transfer.IsDirectory, out var incomingTime);
        var existing = Describe(transfer.DestinationPath, Directory.Exists(transfer.DestinationPath), out var existingTime);
        IncomingDetail = incoming;
        ExistingDetail = existing;
        IncomingIsNewer = incomingTime > existingTime;
    }

    private static string Describe(string path, bool isDirectory, out DateTime modified)
    {
        modified = DateTime.MinValue;
        try
        {
            if (isDirectory)
            {
                var dir = new DirectoryInfo(path);
                if (!dir.Exists) return "missing";
                modified = dir.LastWriteTime;
                return $"folder — modified {modified:g}";
            }

            var file = new FileInfo(path);
            if (!file.Exists) return "missing";
            modified = file.LastWriteTime;
            return $"{ByteSizeFormatter.Format(file.Length)} — modified {modified:g}";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return "unreadable";
        }
    }
}

/// <summary>Backing VM for <see cref="Views.TransferConflictDialog"/>. One resolution is chosen and
/// applied to every conflicting item; Replace is offered only for a move, because a copy is defined
/// as purely additive and must never displace anything.</summary>
public sealed class TransferConflictsViewModel
{
    public IReadOnlyList<TransferConflictViewModel> Items { get; }
    public bool AllowReplace { get; }
    public string Title { get; }
    public string Summary { get; }
    public string ReplaceHint { get; }

    public TransferConflictsViewModel(TransferPlan plan)
    {
        Items = plan.Conflicts.Select(c => new TransferConflictViewModel(c)).ToList();
        AllowReplace = plan.Verb == TransferVerb.Move;

        var folder = Path.GetFileName(plan.DestinationDirectory) is { Length: > 0 } name
            ? name
            : plan.DestinationDirectory;
        Title = Items.Count == 1 ? "An item already exists" : $"{Items.Count:N0} items already exist";
        Summary = Items.Count == 1
            ? $"'{Items[0].Name}' already exists in {folder}."
            : $"{Items.Count:N0} of the items you are moving already exist in {folder}.";
        if (plan.Verb == TransferVerb.Copy)
            Summary = Summary.Replace("moving", "copying", StringComparison.Ordinal);

        ReplaceHint = "Replace keeps the incoming copy. What it displaces is set aside, and Ctrl+Z "
            + "restores both sides until you do something else.";
    }
}
