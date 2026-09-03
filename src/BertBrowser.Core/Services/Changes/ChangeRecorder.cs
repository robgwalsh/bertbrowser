using BertBrowser.Core.Data;

namespace BertBrowser.Core.Services.Changes;

/// <summary>What a recorder needs from its host: where to write, and what never to record.</summary>
/// <param name="ExcludedRootKey">The canonical key of the app's data directory. Every write to the
/// database is itself a filesystem change; without this the log would record its own growth.</param>
public sealed record ChangeRecorderOptions(ChangeLogRepository Repository, string ExcludedRootKey);

/// <summary>
/// Buffers the changes one volume's USN tail resolves and writes them down a batch at a time.
/// One per volume, owned by that volume's tail thread, so nothing here locks.
/// </summary>
/// <remarks>
/// <para>
/// The policy is read once per <see cref="Flush"/>, not cached: it is the user's switch, pushed
/// to the helper over the pipe, and a verb that arrives mid-batch takes effect on the next poll.
/// Off means nothing is buffered and — on the first flush after it was on — the table is wiped, so
/// a batch that was in flight when the user turned recording off never lands.
/// </para>
/// <para>
/// <b>Nothing here throws.</b> <c>MftIndexService.RunVolume</c> catches any exception by ending that
/// volume's tail, silently, which is the right answer for a volume that cannot be read and the
/// wrong one for a change log that cannot be written. A database failure disables this recorder
/// for the session and the index carries on.
/// </para>
/// <para>
/// A flush with nothing buffered touches nothing. Not an optimisation: a writer that opened the
/// database for nothing would tick the journal on every poll, for ever.
/// </para>
/// </remarks>
internal sealed class ChangeRecorder
{
    private readonly ChangeLogRepository _repository;
    private readonly string _excludedRootKey;
    private readonly Func<ChangeLogPolicy> _policy;
    private readonly Func<DateTime> _clock;
    private readonly List<ChangeEvent> _buffer = new();

    private DateTime _lastPrune = DateTime.MinValue;
    private bool _wasEnabled;
    private bool _disabled;

    public ChangeRecorder(ChangeLogRepository repository, string excludedRootKey, Func<ChangeLogPolicy> policy,
        Func<DateTime>? clock = null)
    {
        _repository = repository;
        _excludedRootKey = excludedRootKey;
        _policy = policy;
        _clock = clock ?? (() => DateTime.UtcNow);
    }

    /// <summary>True once a database failure has switched this recorder off for the session.</summary>
    public bool IsDisabled => _disabled;

    public void Add(ChangeEvent change)
    {
        if (_disabled || !_policy().Enabled) return;
        if (ChangeLogRules.IsExcluded(change.PathKey, _excludedRootKey)) return;
        _buffer.Add(change);
    }

    /// <summary>Writes whatever has been buffered since the last call, in one transaction.</summary>
    public void Flush()
    {
        if (_disabled) return;

        var policy = _policy();
        try
        {
            if (!policy.Enabled)
            {
                _buffer.Clear();
                if (_wasEnabled)
                {
                    // The app wipes the table when the user turns recording off; this covers the
                    // batch that was buffered between its wipe and this poll. Once, not on every
                    // poll — nothing else should be deleting rows the app then writes.
                    _repository.Clear();
                    _wasEnabled = false;
                }
                return;
            }

            _wasEnabled = true;
            if (_buffer.Count == 0) return;

            _repository.Record(_buffer);
            _buffer.Clear();

            // Only after a batch, never on an idle poll — the empty-flush rule above. A volume
            // that stays quiet for days therefore never prunes on its own; the view clamps to
            // the retention so nothing is shown past it, and any busy volume prunes the whole
            // table. Do not "fix" this by pruning on idle polls: that is a write per second.
            var now = _clock();
            if (now - _lastPrune >= ChangeLogRules.PruneInterval)
            {
                _repository.Prune(now, policy.Retention);
                _lastPrune = now;
            }
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            // Not only SqliteException: whatever ADO.NET throws on a connection that is not what
            // it should be counts too. The contract is that this never reaches the tail.
            _disabled = true;
            _buffer.Clear();
        }
    }
}
