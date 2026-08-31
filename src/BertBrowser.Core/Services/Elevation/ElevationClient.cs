using System.Text.Json;
using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;
using BertBrowser.Core.Services.Delete;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Core.Services.Elevation;

/// <summary>
/// Running one file operation with an administrator token: raise the prompt, hand the plan to a
/// short-lived helper, stream its progress back, and return an outcome the caller can merge as if
/// nothing unusual had happened.
/// </summary>
public interface IElevatedOperationRunner
{
    /// <summary>Whether the offer should be made at all. See <see cref="IElevationLauncher.CanElevate"/>.</summary>
    bool CanElevate { get; }

    Task<ElevatedRun<TransferOutcome>> RunAsync(
        TransferRetry retry, IProgress<TransferProgress>? progress = null, CancellationToken ct = default);

    Task<ElevatedRun<DeleteOutcome>> RunAsync(
        DeleteRetry retry, IProgress<DeleteProgress>? progress = null, CancellationToken ct = default);

    Task<ElevatedRun<RenameOutcome>> RunAsync(RenameRetry retry, CancellationToken ct = default);

    Task<ElevatedRun<NewItemOutcome>> RunAsync(NewItemRetry retry, CancellationToken ct = default);

    /// <summary>Puts back what an elevated move moved. Takes an outcome rather than a plan, because
    /// that is what <c>TransferExecutor.Undo</c> takes.</summary>
    Task<ElevatedRun<TransferUndoResult>> UndoAsync(TransferOutcome outcome, CancellationToken ct = default);

    /// <inheritdoc cref="UndoAsync(TransferOutcome, CancellationToken)"/>
    Task<ElevatedRun<DeleteUndoResult>> UndoAsync(DeleteOutcome outcome, CancellationToken ct = default);
}

/// <summary>
/// The app's half of the elevated-operation pipe.
/// </summary>
/// <remarks>
/// <para>
/// One session per operation, and the session <em>is</em> the process: create the pipe, raise the
/// prompt, greet, send one request, read until the helper says it is finished, and let it exit. The
/// index client keeps a worker thread alive for the whole session because its helper does; this one
/// has nothing to keep.
/// </para>
/// <para>
/// Nothing here retries on a timer, and nothing here elevates without having been asked. Every
/// attempt is a UAC prompt, which is the same rule the index client follows for the same reason.
/// </para>
/// </remarks>
public sealed class ElevationClient : IElevatedOperationRunner
{
    /// <summary>Generous: the user has to answer a prompt, and the secure desktop takes a moment.</summary>
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromMinutes(2);

    /// <summary>The helper exits on its own once it has answered; this is only to notice it has.</summary>
    private static readonly TimeSpan ExitTimeout = TimeSpan.FromSeconds(5);

    private readonly IElevationLauncher _launcher;
    private readonly IElevationTransportFactory _transports;
    private readonly string _userSid;

    public ElevationClient(IElevationLauncher launcher, IElevationTransportFactory transports, string userSid)
    {
        _launcher = launcher;
        _transports = transports;
        _userSid = userSid;
    }

    public bool CanElevate => _launcher.CanElevate;

    // --- the four operations ---

    public Task<ElevatedRun<TransferOutcome>> RunAsync(
        TransferRetry retry, IProgress<TransferProgress>? progress = null, CancellationToken ct = default)
    {
        var verb = retry.Plan.Verb == TransferVerb.Move
            ? ElevationOperation.TransferMove
            : ElevationOperation.TransferCopy;
        var header = new ElevationHeader(verb, retry.Plan.DestinationDirectory);
        var items = retry.Plan.Transfers.Select(t => new ElevationTransferItem(
            t, Resolution(retry.Resolutions, t.SourcePath)));

        return RunAsync(
            header,
            items.Select(Write),
            p => progress?.Report(new TransferProgress(
                p.Done, p.Total, p.CurrentName, p.BytesDone, p.CurrentItemBytes, p.CurrentItemTotal)),
            collected => new TransferOutcome(
                retry.Plan.Verb,
                retry.Plan.DestinationDirectory,
                [.. collected.Read<ElevationTransferResult>(Collected.Kind.Done)
                    .Select(r => r.Completed).OfType<CompletedTransfer>()],
                [.. collected.Read<ElevationTransferResult>(Collected.Kind.Done)
                    .Select(r => r.Skipped).OfType<string>()],
                [.. collected.Read<FailedTransfer>(Collected.Kind.Fault)],
                collected.Summary?.StagingDirectories ?? [],
                collected.Summary?.Cancelled ?? false),
            ct);
    }

    public Task<ElevatedRun<DeleteOutcome>> RunAsync(
        DeleteRetry retry, IProgress<DeleteProgress>? progress = null, CancellationToken ct = default) =>
        RunAsync(
            new ElevationHeader(ElevationOperation.Delete, DeleteMode: retry.Plan.Mode),
            retry.Plan.Deletions.Select(Write),
            p => progress?.Report(new DeleteProgress(p.Done, p.Total, p.CurrentName)),
            collected => new DeleteOutcome(
                retry.Plan.Permanent,
                [.. collected.Read<DeletedItem>(Collected.Kind.Done)],
                [.. collected.Read<FailedDelete>(Collected.Kind.Fault)],
                collected.Summary?.StagingDirectories ?? []),
            ct);

    public Task<ElevatedRun<RenameOutcome>> RunAsync(RenameRetry retry, CancellationToken ct = default) =>
        RunAsync(
            new ElevationHeader(ElevationOperation.Rename),
            retry.Plan.Renames.Select(Write),
            _ => { },
            collected => new RenameOutcome(
                [.. collected.Read<CompletedRename>(Collected.Kind.Done)],
                [.. collected.Read<FailedRename>(Collected.Kind.Fault)]),
            ct);

    public Task<ElevatedRun<NewItemOutcome>> RunAsync(NewItemRetry retry, CancellationToken ct = default) =>
        RunAsync(
            new ElevationHeader(ElevationOperation.NewItem),
            [Write(retry.Plan)],
            _ => { },
            collected => new NewItemOutcome(
                collected.Read<ElevationNewItemResult>(Collected.Kind.Done).FirstOrDefault()?.CreatedPath,
                collected.Read<FailedNewItem>(Collected.Kind.Fault).FirstOrDefault()),
            ct);

    // --- undoing them ---

    public Task<ElevatedRun<TransferUndoResult>> UndoAsync(
        TransferOutcome outcome, CancellationToken ct = default) =>
        RunAsync(
            new ElevationHeader(ElevationOperation.TransferUndo, outcome.DestinationDirectory),
            outcome.Completed.Select(Write),
            _ => { },
            collected => new TransferUndoResult(
                collected.Summary?.Restored ?? 0,
                [.. collected.Read<FailedTransfer>(Collected.Kind.Fault)]),
            ct);

    public Task<ElevatedRun<DeleteUndoResult>> UndoAsync(
        DeleteOutcome outcome, CancellationToken ct = default) =>
        RunAsync(
            new ElevationHeader(ElevationOperation.DeleteUndo, Permanent: outcome.Permanent),
            outcome.Deleted.Select(Write),
            _ => { },
            collected => new DeleteUndoResult(
                collected.Summary?.Restored ?? 0,
                [.. collected.Read<FailedDelete>(Collected.Kind.Fault)]),
            ct);

    // --- one session ---

    private Task<ElevatedRun<T>> RunAsync<T>(
        ElevationHeader header,
        IEnumerable<string> items,
        Action<ElevationProgressReport> onProgress,
        Func<Collected, T> build,
        CancellationToken ct) =>
        Task.Run(() => Run(header, [.. items], onProgress, build, ct), CancellationToken.None);

    private ElevatedRun<T> Run<T>(
        ElevationHeader header,
        IReadOnlyList<string> items,
        Action<ElevationProgressReport> onProgress,
        Func<Collected, T> build,
        CancellationToken ct)
    {
        if (items.Count == 0) return ElevatedRun<T>.Unavailable("there was nothing to do");
        if (items.Count > ElevationProtocol.MaxItems)
            return ElevatedRun<T>.Unavailable("there are too many items to do at once");
        if (!_launcher.CanElevate) return ElevatedRun<T>.NotAdministrator;

        using var transport = _transports.Create();
        var launch = _launcher.Launch(transport.Endpoint, Environment.ProcessId, _userSid);

        switch (launch.Outcome)
        {
            case ElevationLaunch.Declined: return ElevatedRun<T>.Declined;
            case ElevationLaunch.NotAdministrator: return ElevatedRun<T>.NotAdministrator;
            case ElevationLaunch.Failed: return ElevatedRun<T>.Unavailable(launch.Detail);
        }

        using var stream = transport.Accept(launch.ProcessId, ConnectTimeout);
        if (stream is null) return ElevatedRun<T>.Unavailable("the helper did not connect");

        try
        {
            var collected = Converse(stream, header, items, onProgress, ct);
            if (collected.Fatal is { } fatal) return ElevatedRun<T>.Unavailable(fatal);
            if (collected.Summary is null) return ElevatedRun<T>.Unavailable("the helper stopped part-way");

            return ElevatedRun<T>.Ran(build(collected));
        }
        finally
        {
            _launcher.WaitForExit(launch.ProcessId, ExitTimeout);
        }
    }

    private static Collected Converse(
        Stream stream,
        ElevationHeader header,
        IReadOnlyList<string> items,
        Action<ElevationProgressReport> onProgress,
        CancellationToken ct)
    {
        var reader = new LineReader(stream, NavigationRequest.MaxLineLength);
        var collected = new Collected();
        var writeGate = new Lock();

        void Send(ElevationVerb verb, string payload = "")
        {
            lock (writeGate)
            {
                try
                {
                    LineChannel.WriteLine(stream, ElevationProtocol.Format(new ElevationMessage(verb, payload)));
                }
                catch (Exception ex) when (ex is IOException or ObjectDisposedException)
                {
                    // The helper has gone; the read below will see it too and say so once.
                }
            }
        }

        // Registered rather than polled, so a cancel lands while the helper is mid-file rather than
        // between items. The helper's executors do the rest: nothing half-written, and whatever got
        // across stays across.
        using var cancellation = ct.Register(() => Send(ElevationVerb.Cancel));

        Send(ElevationVerb.Hello, ElevationProtocol.ProtocolVersion.ToString());

        var sent = false;
        while (true)
        {
            string? line;
            try
            {
                line = reader.ReadLine();
            }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException)
            {
                line = null;
            }

            if (line is null) return collected;
            if (!ElevationProtocol.TryParse(line, out var message)) continue;

            switch (message.Verb)
            {
                case ElevationVerb.Hello
                    when ElevationProtocol.VersionOf(message) != ElevationProtocol.ProtocolVersion:
                    collected.Fatal = "the helper speaks a different version of this protocol";
                    return collected;

                case ElevationVerb.Ready when !sent:
                    sent = true;
                    Send(ElevationVerb.Begin, Write(header));
                    foreach (var item in items) Send(ElevationVerb.Item, item);
                    Send(ElevationVerb.Go);
                    break;

                case ElevationVerb.Progress
                    when Read<ElevationProgressReport>(message.Payload) is { } report:
                    onProgress(report);
                    break;

                case ElevationVerb.Done:
                    collected.Done.Add(message.Payload);
                    break;

                case ElevationVerb.Fault:
                    collected.Fault.Add(message.Payload);
                    break;

                case ElevationVerb.End:
                    collected.Summary = Read<ElevationSummary>(message.Payload);
                    return collected;

                case ElevationVerb.Fatal:
                    collected.Fatal = message.Payload;
                    return collected;
            }
        }
    }

    private static ConflictResolution Resolution(
        IReadOnlyDictionary<string, ConflictResolution> resolutions, string source)
    {
        var key = ElevationRules.KeyOf(source);
        return key is not null && resolutions.TryGetValue(key, out var resolution)
            ? resolution
            : ConflictResolution.KeepBoth;
    }

    private static string Write<T>(T value) => JsonSerializer.Serialize(value, ElevationJson.Options);

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

    /// <summary>What the helper said, before it is turned back into an outcome.</summary>
    private sealed class Collected
    {
        internal enum Kind { Done, Fault }

        internal List<string> Done { get; } = [];
        internal List<string> Fault { get; } = [];
        internal ElevationSummary? Summary { get; set; }
        internal string? Fatal { get; set; }

        /// <summary>The lines of one kind that parse. One that does not is dropped rather than
        /// throwing: a malformed line from the helper must not lose the ones around it.</summary>
        internal IEnumerable<T> Read<T>(Kind kind) =>
            (kind == Kind.Done ? Done : Fault)
                .Select(ElevationClient.Read<T>)
                .OfType<T>();
    }
}
