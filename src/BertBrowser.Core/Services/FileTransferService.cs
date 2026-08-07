using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services;

public interface IFileTransferService
{
    /// <summary>
    /// Copies a file or directory into <paramref name="destinationDir"/>, generating a
    /// "name (2)"-style unique name on collision. Returns the destination path.
    /// </summary>
    string CopyInto(string sourcePath, string destinationDir);

    /// <summary>
    /// Moves a file or directory into <paramref name="destinationDir"/>. Returns the
    /// destination path, or <paramref name="sourcePath"/> unchanged when the move is a
    /// no-op (source already lives in the destination directory).
    /// </summary>
    string MoveInto(string sourcePath, string destinationDir);
}

/// <summary>
/// Single-item transfers for the clipboard (cut/copy/paste). A thin facade over
/// <see cref="TransferPlanner"/> and <see cref="TransferExecutor"/> so paste and drag-and-drop
/// share one audited implementation of the rules that keep data intact, rather than each having
/// their own. Collisions always resolve to <see cref="ConflictResolution.KeepBoth"/> here: paste
/// has no dialog to ask through, so it must never displace anything.
/// </summary>
public sealed class FileTransferService : IFileTransferService
{
    private readonly TransferPlanner _planner;
    private readonly TransferExecutor _executor;

    public FileTransferService() : this(new FileSystemTransferProbe())
    {
    }

    public FileTransferService(ITransferProbe probe)
    {
        _planner = new TransferPlanner(probe);
        _executor = new TransferExecutor(probe);
    }

    public string CopyInto(string sourcePath, string destinationDir) =>
        Transfer(sourcePath, destinationDir, TransferVerb.Copy);

    public string MoveInto(string sourcePath, string destinationDir) =>
        Transfer(sourcePath, destinationDir, TransferVerb.Move);

    private string Transfer(string sourcePath, string destinationDir, TransferVerb verb)
    {
        var plan = _planner.Plan([sourcePath], destinationDir, verb);

        if (plan.Rejected.Count > 0)
        {
            var rejected = plan.Rejected[0];
            // A source already sitting in the destination is the one refusal that is not an error.
            if (rejected.Reason == TransferRejection.AlreadyInDestination) return sourcePath;
            throw Rejection(sourcePath, rejected);
        }

        var outcome = _executor.Execute(plan);
        if (outcome.Failed.Count > 0)
            throw new IOException(outcome.Failed[0].Message);
        if (outcome.Completed.Count == 0)
            throw new IOException($"'{Path.GetFileName(sourcePath)}' could not be transferred.");

        return outcome.Completed[0].FinalPath;
    }

    private static Exception Rejection(string sourcePath, RejectedTransfer rejected) => rejected.Reason switch
    {
        TransferRejection.SourceMissing =>
            new FileNotFoundException($"Source not found: {sourcePath}", sourcePath),
        TransferRejection.SourceIsRoot =>
            new InvalidOperationException("Cannot move a drive root."),
        TransferRejection.DestinationMissing =>
            new DirectoryNotFoundException(rejected.Message),
        TransferRejection.DestinationNotDirectory =>
            new DirectoryNotFoundException(rejected.Message),
        TransferRejection.DestinationIsSource or TransferRejection.DestinationInsideSource =>
            new InvalidOperationException(
                $"Cannot copy or move '{Path.GetFileName(sourcePath)}' into itself or one of its subfolders."),
        _ => new InvalidOperationException(rejected.Message),
    };
}
