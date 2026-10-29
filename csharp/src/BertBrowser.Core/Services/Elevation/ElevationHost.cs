using System.Text.Json;
using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Elevation;

/// <summary>
/// The elevated end of one file operation: reads a single request, carries it out through the same
/// Core executors the app uses, reports what happened, and returns.
/// </summary>
/// <remarks>
/// <para>
/// <b>It hosts the real executors, not a narrower set of primitives, and that is the safety
/// argument rather than a convenience.</b> Every invariant that matters lives inside them — nothing
/// deleted to make room, a cross-volume move that copies, verifies and only then deletes, junction
/// trees refused, <c>DirectoryRemoval.RemoveTree</c> instead of <c>Directory.Delete(recursive)</c>,
/// the staging-folder name guard, <c>ProtectedLocations</c>, and every planner rule re-applied
/// against live disk before a byte moves. A helper that took "copy this file" primitives would be
/// <em>following instructions</em> from a medium-integrity peer; hosting the executors means the
/// process doing the dangerous thing is the process re-checking the rules.
/// </para>
/// <para>
/// <b>One request, then it returns</b>, and that is held twice over rather than once.
/// <see cref="ElevationVerb.Begin"/> is accepted only when there is no header yet and
/// <see cref="ElevationVerb.Item"/> only before <see cref="ElevationVerb.Go"/>; after <c>Go</c> the
/// only verb read is <see cref="ElevationVerb.Cancel"/>, and the pipe is closed as the work
/// finishes. So a peer that somehow owned the pipe can neither add a path to a running operation nor
/// start a second one — and neither guard is load-bearing alone, which is why
/// <c>ASecondHeaderIsRefused</c> asserts the state machine directly instead of inferring it from
/// what did or did not happen on disk.
/// </para>
/// <para>
/// The executor runs on a worker thread while this thread stays in the read loop. That is not
/// tidiness: a <c>Cancel</c> arriving while the main thread sat inside <c>Execute</c> would never be
/// seen, and — as the index helper's pipe already records — a non-overlapped handle serialises I/O,
/// so a blocking read would also block the worker's progress writes on the same handle.
/// </para>
/// </remarks>
public sealed class ElevationHost
{
    private readonly Stream _stream;
    private readonly IRecycleBin? _recycleBin;
    private readonly IRecycleProbe? _recycleProbe;
    private readonly Action<string>? _grantAccess;
    private readonly Lock _writeGate = new();

    /// <param name="grantAccess">Called for every staging folder the run created, so the process
    /// that launched this one can still commit and purge it. A folder an elevated process makes at a
    /// volume root inherits that root's ACL, which grants ordinary users read and not delete — so
    /// without this an elevated staged delete leaves a holding folder the app can neither erase nor
    /// undo from. The database's own note about Administrators-owned files does not cover this: that
    /// reasoning is about the profile, which grants the interactive user inheritable full control,
    /// and a volume root does not.</param>
    public ElevationHost(
        Stream stream,
        IRecycleBin? recycleBin = null,
        IRecycleProbe? recycleProbe = null,
        Action<string>? grantAccess = null)
    {
        _stream = stream;
        _recycleBin = recycleBin;
        _recycleProbe = recycleProbe;
        _grantAccess = grantAccess;
    }

    /// <summary>Serves one request and returns. A null line — the app has gone — ends it too.</summary>
    public void Run(CancellationToken ct = default)
    {
        var reader = new LineReader(_stream, NavigationRequest.MaxLineLength);

        Send(ElevationVerb.Hello, ElevationProtocol.ProtocolVersion.ToString());
        Send(ElevationVerb.Ready);

        ElevationHeader? header = null;
        var items = new List<string>();

        while (true)
        {
            var line = reader.ReadLine();
            if (line is null) return;
            if (!ElevationProtocol.TryParse(line, out var message)) continue;

            switch (message.Verb)
            {
                case ElevationVerb.Hello:
                    if (ElevationProtocol.VersionOf(message) != ElevationProtocol.ProtocolVersion)
                    {
                        Send(ElevationVerb.Fatal, "Version mismatch.");
                        return;
                    }
                    break;

                case ElevationVerb.Begin when header is null:
                    header = Read<ElevationHeader>(message.Payload);
                    if (header is null)
                    {
                        Send(ElevationVerb.Fatal, "The request header could not be read.");
                        return;
                    }
                    break;

                case ElevationVerb.Item when header is not null:
                    if (items.Count >= ElevationProtocol.MaxItems)
                    {
                        // Refused whole rather than truncated: a partial request is a different
                        // operation from the one the user consented to.
                        Send(ElevationVerb.Fatal, "The request names more items than this helper accepts.");
                        return;
                    }
                    items.Add(message.Payload);
                    break;

                case ElevationVerb.Go when header is not null:
                    Carry(header, items, ct, reader);
                    return;

                default:
                    Send(ElevationVerb.Fatal, "That is not something this helper accepts here.");
                    return;
            }
        }
    }

    // --- carrying out the one request ---

    private void Carry(
        ElevationHeader header, IReadOnlyList<string> items, CancellationToken ct, LineReader reader)
    {
        using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(ct);

        var worker = new Thread(() =>
        {
            try
            {
                Perform(header, items, cancellation.Token);
            }
            catch (Exception ex)
            {
                // Reported from in here rather than after the join, because the finally below closes
                // the stream: a Fatal sent afterwards would be swallowed as a broken pipe and the app
                // would see the helper simply vanish. An exception left to escape this thread would
                // be unobserved and take the process down mid-operation.
                Send(ElevationVerb.Fatal, ElevationProtocol.Summarize(ex.Message));
            }
            finally
            {
                // Unparks the read loop below, which is otherwise waiting for a Cancel that is now
                // never coming.
                try { _stream.Dispose(); } catch (Exception ex) when (IsPipeGone(ex)) { }
            }
        })
        {
            IsBackground = true,
            Name = "bertbrowser elevated operation",
        };
        worker.Start();

        while (true)
        {
            string? line;
            try
            {
                line = reader.ReadLine();
            }
            catch (Exception ex) when (IsPipeGone(ex))
            {
                line = null;
            }

            // A null line is the app gone or the worker finished and closed the stream. Either way
            // there is nothing more to read; cancelling is right in both cases, and harmless in the
            // second because the work is already done.
            if (line is null) break;
            if (!ElevationProtocol.TryParse(line, out var message)) continue;
            if (message.Verb != ElevationVerb.Cancel) continue;

            cancellation.Cancel();
            break;
        }

        worker.Join();
    }

    private void Perform(ElevationHeader header, IReadOnlyList<string> items, CancellationToken ct)
    {
        switch (header.Operation)
        {
            case ElevationOperation.TransferMove or ElevationOperation.TransferCopy:
                Transfer(header, items, ct);
                break;

            case ElevationOperation.TransferUndo:
                UndoTransfer(header, items);
                break;

            case ElevationOperation.Delete:
                DeleteItems(header, items, ct);
                break;

            case ElevationOperation.DeleteUndo:
                UndoDelete(header, items);
                break;

            case ElevationOperation.Rename:
                RenameItems(items);
                break;

            case ElevationOperation.NewItem:
                CreateItem(items);
                break;

            default:
                Send(ElevationVerb.Fatal, "That is not an operation this helper knows.");
                break;
        }
    }

    private void Transfer(ElevationHeader header, IReadOnlyList<string> items, CancellationToken ct)
    {
        var parsed = ReadAll<ElevationTransferItem>(items);
        if (parsed is null || !AllAcceptable(parsed.Select(p => p.Item.SourcePath)) ||
            !NavigationRequest.IsAcceptablePath(header.DestinationDirectory))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        var verb = header.Operation == ElevationOperation.TransferMove
            ? TransferVerb.Move
            : TransferVerb.Copy;
        var plan = new TransferPlan(
            verb, header.DestinationDirectory, [.. parsed.Select(p => p.Item)], []);
        var resolutions = parsed.ToDictionary(
            p => Paths.PathKey.Canonicalize(p.Item.SourcePath), p => p.Resolution, StringComparer.Ordinal);

        var outcome = new TransferExecutor().Execute(
            plan, resolutions, ct, new SendingProgress<TransferProgress>(Report));

        foreach (var done in outcome.Completed)
            Send(ElevationVerb.Done, Write(new ElevationTransferResult(done, null)));
        foreach (var skipped in outcome.Skipped)
            Send(ElevationVerb.Done, Write(new ElevationTransferResult(null, skipped)));
        foreach (var fault in outcome.Failed)
            Send(ElevationVerb.Fault, Write(fault));

        Finish(outcome.Cancelled, outcome.StagingDirectories);
    }

    private void UndoTransfer(ElevationHeader header, IReadOnlyList<string> items)
    {
        var parsed = ReadAll<CompletedTransfer>(items);
        if (parsed is null || !AllAcceptable(parsed.Select(p => p.SourcePath)) ||
            !AllAcceptable(parsed.Select(p => p.FinalPath)))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        // StagingDirectories: [] deliberately. The unelevated half still holds items in its own
        // staging folders, and this pass must not purge them out from under it.
        var outcome = new TransferOutcome(
            TransferVerb.Move, header.DestinationDirectory, parsed, [], [], []);
        var result = new TransferExecutor().Undo(outcome);

        foreach (var fault in result.Failed) Send(ElevationVerb.Fault, Write(fault));
        Finish(cancelled: false, [], result.Restored);
    }

    private void DeleteItems(ElevationHeader header, IReadOnlyList<string> items, CancellationToken ct)
    {
        var parsed = ReadAll<PlannedDelete>(items);
        if (parsed is null || !AllAcceptable(parsed.Select(p => p.SourcePath)))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        var outcome = Deleter().Execute(
            new DeletePlan(header.DeleteMode, parsed, []),
            ct,
            new SendingProgress<DeleteProgress>(Report));

        foreach (var done in outcome.Deleted) Send(ElevationVerb.Done, Write(done));
        foreach (var fault in outcome.Failed) Send(ElevationVerb.Fault, Write(fault));

        Finish(cancelled: false, outcome.StagingDirectories);
    }

    private void UndoDelete(ElevationHeader header, IReadOnlyList<string> items)
    {
        var parsed = ReadAll<DeletedItem>(items);
        if (parsed is null || !AllAcceptable(parsed.Select(p => p.SourcePath)))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        var result = Deleter().Undo(new DeleteOutcome(header.Permanent, parsed, [], []));

        foreach (var fault in result.Failed) Send(ElevationVerb.Fault, Write(fault));
        Finish(cancelled: false, [], result.Restored);
    }

    private void RenameItems(IReadOnlyList<string> items)
    {
        var parsed = ReadAll<PlannedRename>(items);
        if (parsed is null || !AllAcceptable(parsed.Select(p => p.SourcePath)) ||
            !AllAcceptable(parsed.Select(p => p.TargetPath)))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        var outcome = new RenameExecutor().Execute(new RenamePlan(parsed, []));

        foreach (var done in outcome.Completed) Send(ElevationVerb.Done, Write(done));
        foreach (var fault in outcome.Failed) Send(ElevationVerb.Fault, Write(fault));

        Finish(cancelled: false, []);
    }

    private void CreateItem(IReadOnlyList<string> items)
    {
        var parsed = ReadAll<NewItemPlan>(items);
        if (parsed is not [var plan] ||
            !NavigationRequest.IsAcceptablePath(plan.Directory) ||
            (plan.TemplatePath is { } template && !NavigationRequest.IsAcceptablePath(template)))
        {
            Send(ElevationVerb.Fatal, "The request named something that is not an acceptable path.");
            return;
        }

        var outcome = new NewItemExecutor().Execute(plan);

        if (outcome.CreatedPath is { } created)
            Send(ElevationVerb.Done, Write(new ElevationNewItemResult(created)));
        if (outcome.Failed is { } failure)
            Send(ElevationVerb.Fault, Write(failure));

        Finish(cancelled: false, []);
    }

    private DeleteExecutor Deleter() =>
        new(new FileSystemDeleteProbe(), protectedPaths: null, stagingRoot: null, _recycleBin, _recycleProbe);

    private void Finish(bool cancelled, IReadOnlyList<string> staging, int restored = 0)
    {
        foreach (var directory in staging) _grantAccess?.Invoke(directory);
        Send(ElevationVerb.End, Write(new ElevationSummary(cancelled, staging, restored)));
    }

    // --- the wire ---

    private void Report(TransferProgress progress) =>
        Send(ElevationVerb.Progress, Write(new ElevationProgressReport(
            progress.Done, progress.Total, progress.CurrentName,
            progress.BytesDone, progress.CurrentItemBytes, progress.CurrentItemTotal)));

    private void Report(DeleteProgress progress) =>
        Send(ElevationVerb.Progress, Write(new ElevationProgressReport(
            progress.Done, progress.Total, progress.CurrentName)));

    private void Send(ElevationVerb verb, string payload = "")
    {
        lock (_writeGate)
        {
            try
            {
                LineChannel.WriteLine(_stream, ElevationProtocol.Format(new ElevationMessage(verb, payload)));
            }
            catch (Exception ex) when (IsPipeGone(ex))
            {
                // The app has gone. Losing the pipe is a cancel with nobody left to tell, and the
                // executors' own guarantees mean nothing is half-written.
            }
        }
    }

    private static bool AllAcceptable(IEnumerable<string> paths) =>
        paths.All(NavigationRequest.IsAcceptablePath);

    private static bool IsPipeGone(Exception ex) => ex is IOException or ObjectDisposedException;

    internal static string Write<T>(T value) => JsonSerializer.Serialize(value, ElevationJson.Options);

    private static T? Read<T>(string payload)
    {
        try
        {
            return JsonSerializer.Deserialize<T>(payload, ElevationJson.Options);
        }
        catch (JsonException)
        {
            return default;
        }
    }

    /// <summary>Every item, or null if any one of them could not be read — a malformed request is
    /// refused whole rather than carried out in part.</summary>
    private static List<T>? ReadAll<T>(IReadOnlyList<string> payloads)
    {
        var parsed = new List<T>(payloads.Count);
        foreach (var payload in payloads)
        {
            if (Read<T>(payload) is not { } item) return null;
            parsed.Add(item);
        }
        return parsed;
    }

    /// <summary>An <see cref="IProgress{T}"/> that writes to the pipe. Not
    /// <c>System.Progress&lt;T&gt;</c>: that one posts to the captured synchronization context, and
    /// there is no dispatcher here — the reports would arrive on the thread pool in no particular
    /// order.</summary>
    private sealed class SendingProgress<T>(Action<T> send) : IProgress<T>
    {
        public void Report(T value) => send(value);
    }
}

/// <summary>One options object for both ends, so they cannot disagree about how a record is
/// written.</summary>
internal static class ElevationJson
{
    internal static readonly JsonSerializerOptions Options = new()
    {
        // Compact and on one line: the framing depends on the payload carrying no raw newline, and
        // indenting would put one in.
        WriteIndented = false,
    };
}
