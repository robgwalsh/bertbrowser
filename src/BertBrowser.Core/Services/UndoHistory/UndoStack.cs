namespace BertBrowser.Core.Services.UndoHistory;

/// <summary>One operation in the history, and what has happened to it since.</summary>
public sealed class UndoEntry
{
    internal UndoEntry(int id, IUndoableRecord record, HeldSize held, DateTime nowUtc)
    {
        Id = id;
        Record = record;
        Held = held;
        CreatedUtc = nowUtc;
        ChangedUtc = nowUtc;
    }

    /// <summary>Unique for the session, and never reused, so a row or a script can name an entry
    /// across steps that replace its record.</summary>
    public int Id { get; }

    public IUndoableRecord Record { get; internal set; }

    /// <summary>In effect: the next step on it would be an undo.</summary>
    public bool IsDone => Record.IsApplied;

    public HeldSize Held { get; internal set; }

    public DateTime CreatedUtc { get; }

    /// <summary>When it was last done, undone or redone.</summary>
    public DateTime ChangedUtc { get; internal set; }

    /// <summary>The most recent step taken on it, and how it went; null for an entry that has only
    /// ever been done.</summary>
    public UndoDirection? LastDirection { get; internal set; }

    public StepReport? LastReport { get; internal set; }
}

/// <summary>What leaving the history took with it. The caller owes each entry's record one
/// <see cref="IUndoableRecord.Retire"/>, off the UI thread — that is the moment the data goes.</summary>
public sealed record Released(IReadOnlyList<UndoEntry> Entries)
{
    public static Released Nothing { get; } = new([]);

    public HeldSize Held => Entries.Aggregate(HeldSize.None, (sum, e) => sum + e.Held);

    public void RetireAll()
    {
        foreach (var entry in Entries) entry.Record.Retire();
    }
}

/// <summary>
/// The undo history: every operation this session can still take back or do again, oldest first,
/// with a cursor between what is in effect and what has been undone.
/// </summary>
/// <remarks>
/// <para>
/// <b>It never touches disk.</b> Everything that commits data comes back as a <see cref="Released"/>
/// for the caller to retire, which is what lets every rule here be tested with fake records.
/// </para>
/// <para>
/// Entries <c>[0, Cursor)</c> are done and <c>[Cursor, Count)</c> are undone — strictly, since only
/// the entry either side of the cursor may take a step. A new operation discards everything after
/// the cursor: there is one line of history, as in every editor, and the branch a new action
/// abandons is not something anybody can get back to.
/// </para>
/// <para>
/// <b>The budget releases the oldest done entry first, then the furthest undone one — and never the
/// next undo.</b> A single delete larger than the whole budget stays undoable, as it did when there
/// was one slot; the history is simply one entry deep until something newer arrives. Held bytes that
/// could only partly be measured count as what was measured, which is a floor, never as nothing.
/// </para>
/// </remarks>
public sealed class UndoStack
{
    private readonly List<UndoEntry> _entries = [];
    private int _nextId = 1;

    public UndoStack(UndoBudget budget) => Budget = budget;

    public UndoBudget Budget { get; private set; }

    /// <summary>Oldest first.</summary>
    public IReadOnlyList<UndoEntry> Entries => _entries;

    /// <summary>How many entries are done; the index of the first undone one.</summary>
    public int Cursor { get; private set; }

    public UndoEntry? NextUndo => Cursor > 0 ? _entries[Cursor - 1] : null;

    public UndoEntry? NextRedo => Cursor < _entries.Count ? _entries[Cursor] : null;

    public HeldSize Held => _entries.Aggregate(HeldSize.None, (sum, e) => sum + e.Held);

    /// <summary>How many entries have left the history to stay within the budget this session.</summary>
    public int ReleasedCount { get; private set; }

    /// <summary>What those entries were holding when they went.</summary>
    public long ReleasedBytes { get; private set; }

    /// <summary>Adds a new operation after the cursor, discarding the redo branch first.</summary>
    public (UndoEntry Entry, Released Released) Push(IUndoableRecord record, HeldSize held, DateTime nowUtc)
    {
        if (!record.IsApplied)
            throw new ArgumentException("A new operation is always in effect.", nameof(record));

        var released = new List<UndoEntry>(_entries.Skip(Cursor));
        _entries.RemoveRange(Cursor, _entries.Count - Cursor);

        var entry = new UndoEntry(_nextId++, record, held, nowUtc);
        _entries.Add(entry);
        Cursor = _entries.Count;

        Evict(released);
        return (entry, new Released(released));
    }

    /// <summary>What undoing back to <paramref name="target"/> would run, next undo first.</summary>
    public IReadOnlyList<UndoEntry> UndoSpan(UndoEntry target)
    {
        var index = IndexOf(target);
        if (index >= Cursor) throw new ArgumentException("That entry is not done.", nameof(target));
        return [.. Enumerable.Range(index, Cursor - index).Reverse().Select(i => _entries[i])];
    }

    /// <summary>What redoing forward to <paramref name="target"/> would run, next redo first.</summary>
    public IReadOnlyList<UndoEntry> RedoSpan(UndoEntry target)
    {
        var index = IndexOf(target);
        if (index < Cursor) throw new ArgumentException("That entry is not undone.", nameof(target));
        return [.. Enumerable.Range(Cursor, index - Cursor + 1).Select(i => _entries[i])];
    }

    /// <summary>
    /// Records a step that moved something: the entry swaps to <paramref name="next"/> and the
    /// cursor passes it. Returns anything the new held size pushed over the budget.
    /// </summary>
    public Released Applied(UndoEntry entry, IUndoableRecord next, HeldSize held, StepReport report, DateTime nowUtc)
    {
        var direction = Direction(entry);
        if (next.IsApplied != (direction == UndoDirection.Redo))
            throw new ArgumentException("The step did not change the entry's state.", nameof(next));

        entry.Record = next;
        entry.Held = held;
        entry.LastDirection = direction;
        entry.LastReport = report;
        entry.ChangedUtc = nowUtc;
        Cursor += direction == UndoDirection.Undo ? -1 : 1;

        var released = new List<UndoEntry>();
        Evict(released);
        return new Released(released);
    }

    /// <summary>Records a step that moved nothing. The entry stays where it was, and says why.</summary>
    public void Refused(UndoEntry entry, StepReport report, DateTime nowUtc)
    {
        entry.LastDirection = Direction(entry);
        entry.LastReport = report;
        entry.ChangedUtc = nowUtc;
    }

    public Released SetBudget(UndoBudget budget)
    {
        Budget = budget;
        var released = new List<UndoEntry>();
        Evict(released);
        return new Released(released);
    }

    /// <summary>Empties the history. Not counted as released for the budget's sake — the footer's
    /// "released" figure is about what the limits took, not what the user asked to let go.</summary>
    public Released Clear()
    {
        var released = new Released([.. _entries]);
        _entries.Clear();
        Cursor = 0;
        return released;
    }

    /// <summary>Which way the next step on <paramref name="entry"/> goes. Only the two entries either
    /// side of the cursor can take one.</summary>
    public UndoDirection Direction(UndoEntry entry)
    {
        if (ReferenceEquals(entry, NextUndo)) return UndoDirection.Undo;
        if (ReferenceEquals(entry, NextRedo)) return UndoDirection.Redo;
        throw new ArgumentException("Only the entry either side of the cursor can take a step.", nameof(entry));
    }

    private int IndexOf(UndoEntry entry)
    {
        var index = _entries.IndexOf(entry);
        if (index < 0) throw new ArgumentException("That entry is no longer in the history.", nameof(entry));
        return index;
    }

    private void Evict(List<UndoEntry> released)
    {
        while (OverBudget())
        {
            int victim;
            if (Cursor > 1) victim = 0;                                // oldest done, not the next undo
            else if (_entries.Count > Cursor) victim = _entries.Count - 1; // furthest undone
            else break;                                                // only the next undo is left

            var entry = _entries[victim];
            _entries.RemoveAt(victim);
            if (victim < Cursor) Cursor--;
            released.Add(entry);

            ReleasedCount++;
            ReleasedBytes += entry.Held.Bytes;
        }
    }

    private bool OverBudget() =>
        _entries.Count > Budget.MaxEntries || Held.Bytes > Budget.MaxHeldBytes;
}
