using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.Harness;

/// <summary>
/// Answers the conflict dialog from the script instead of from a person.
/// </summary>
/// <remarks>
/// <para>
/// The real one is a modal, which a run can neither see nor dismiss — the same problem
/// <c>RecordingUserNotice</c> and <c>RecordingUserConfirm</c> solve for the other two questions the
/// shell asks. Everything above this is real: the plan, the per-path map, the executor.
/// </para>
/// <para>
/// <b>Keep both for everything is the default</b>, which is exactly what every transfer got before
/// there was a prompt at all — so a script that says nothing about conflicts behaves as it always
/// did. The <c>conflict</c> command changes the answer, either for the whole drop or per name.
/// </para>
/// </remarks>
internal sealed class ScriptedConflictPrompt : IConflictPrompt
{
    private readonly Lock _gate = new();
    private ConflictResolution _all = ConflictResolution.KeepBoth;
    private Dictionary<string, ConflictResolution> _byName = new(StringComparer.OrdinalIgnoreCase);
    private bool _cancel;
    private int _asked;

    /// <summary>How many times a transfer has actually had to ask. The whole point of
    /// <c>assert-conflicts</c>: a paste that silently kept both used to ask zero times.</summary>
    internal int Asked
    {
        get { lock (_gate) return _asked; }
    }

    internal void AnswerAll(ConflictResolution resolution)
    {
        lock (_gate)
        {
            _all = resolution;
            _byName.Clear();
            _cancel = false;
        }
    }

    internal void AnswerByName(IReadOnlyDictionary<string, ConflictResolution> answers)
    {
        lock (_gate)
        {
            _byName = answers.ToDictionary(
                a => Normalize(a.Key), a => a.Value, StringComparer.OrdinalIgnoreCase);
            _cancel = false;
        }
    }

    /// <summary>Poses the user closing the dialog, which abandons the whole transfer.</summary>
    internal void AnswerCancel()
    {
        lock (_gate)
        {
            _cancel = true;
            _byName.Clear();
        }
    }

    /// <summary>The rows the last ask put up, by label, with the answer each one started on.
    /// What <c>assert-conflict-rows</c> and <c>assert-conflict-default</c> read.</summary>
    internal IReadOnlyDictionary<string, ConflictResolution> LastOffered
    {
        get { lock (_gate) return _lastOffered; }
    }

    private Dictionary<string, ConflictResolution> _lastOffered = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Records what a plan <em>would</em> put up, without counting it as an ask. So
    /// <c>conflict-plan</c> — which builds the dialog's plan but never raises it — and a real
    /// transfer both leave the rows in one place for the assertions to read.
    /// </summary>
    internal void Offer(TransferPlan plan)
    {
        lock (_gate) _lastOffered = Rows(plan);
    }

    private static Dictionary<string, ConflictResolution> Rows(TransferPlan plan) =>
        plan.Conflicts.ToDictionary(plan.LabelFor, ConflictDefaults.For, StringComparer.OrdinalIgnoreCase);

    public IReadOnlyDictionary<string, ConflictResolution>? Ask(TransferPlan plan)
    {
        lock (_gate)
        {
            _asked++;
            _lastOffered = Rows(plan);

            if (_cancel) return null;

            return plan.Conflicts.ToDictionary(
                t => PathKey.Canonicalize(t.SourcePath),
                t => Answer(plan, t));
        }
    }

    /// <summary>
    /// The label first, then the leaf name; anything unnamed gets whatever <c>conflict</c> last set
    /// for the drop as a whole.
    /// </summary>
    /// <remarks>
    /// After a merge two rows can share a leaf — that is the whole reason rows are named by their
    /// path relative to the drop — so a script saying <c>photos\2024\a.jpg=replace</c> has to be
    /// able to reach exactly one of them. A bare name still works, which is what keeps every script
    /// written before merging existed behaving as it did.
    /// </remarks>
    private ConflictResolution Answer(TransferPlan plan, PlannedTransfer transfer)
    {
        if (_byName.TryGetValue(Normalize(plan.LabelFor(transfer)), out var byLabel)) return byLabel;
        if (_byName.TryGetValue(transfer.Name, out var byLeaf)) return byLeaf;
        return _all;
    }

    /// <summary>Scripts may spell a relative path either way round; the label is always '\'.</summary>
    internal static string Normalize(string name) => name.Replace('/', '\\');
}
