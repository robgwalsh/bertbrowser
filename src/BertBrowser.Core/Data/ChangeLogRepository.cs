using BertBrowser.Core.Paths;
using BertBrowser.Core.Services.Changes;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Core.Data;

/// <summary>
/// Persistence for <c>fs_change</c>, the change timeline. Synchronous ADO.NET with a pooled
/// connection per method, like the other repositories; the writer is the index helper's USN tail
/// and the reader is the app, in two processes over one WAL database.
/// </summary>
public sealed class ChangeLogRepository
{
    private readonly Db _db;

    public ChangeLogRepository(Db db) => _db = db;

    /// <summary>
    /// Writes one drained batch in one transaction, folding each event into the most recent row
    /// for the same path and kind when that row is inside <see cref="ChangeLogRules.CoalesceWindow"/>.
    /// </summary>
    /// <remarks>
    /// An empty batch opens no connection. That is not an optimisation: every write here is itself
    /// a filesystem change the tail will see on its next poll, and a writer that touched the
    /// database for nothing would tick the journal for ever.
    /// </remarks>
    public void Record(IReadOnlyList<ChangeEvent> events)
    {
        if (events.Count == 0) return;

        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();

        using var fold = conn.CreateCommand();
        fold.Transaction = tx;
        fold.CommandText =
            """
            UPDATE fs_change SET count = count + 1, last_utc = @utc
            WHERE id = (SELECT id FROM fs_change
                        WHERE path_key = @key AND kind = @kind AND last_utc >= @windowStart
                        ORDER BY last_utc DESC LIMIT 1);
            """;
        var fKey = fold.Parameters.Add("@key", SqliteType.Text);
        var fKind = fold.Parameters.Add("@kind", SqliteType.Integer);
        var fUtc = fold.Parameters.Add("@utc", SqliteType.Text);
        var fWindow = fold.Parameters.Add("@windowStart", SqliteType.Text);

        using var insert = conn.CreateCommand();
        insert.Transaction = tx;
        insert.CommandText =
            """
            INSERT INTO fs_change(path_key, display_path, old_path, is_dir, hidden, kind, first_utc, last_utc, count)
            VALUES (@key, @display, @old, @isDir, @hidden, @kind, @utc, @utc, 1);
            """;
        var iKey = insert.Parameters.Add("@key", SqliteType.Text);
        var iDisplay = insert.Parameters.Add("@display", SqliteType.Text);
        var iOld = insert.Parameters.Add("@old", SqliteType.Text);
        var iIsDir = insert.Parameters.Add("@isDir", SqliteType.Integer);
        var iHidden = insert.Parameters.Add("@hidden", SqliteType.Integer);
        var iKind = insert.Parameters.Add("@kind", SqliteType.Integer);
        var iUtc = insert.Parameters.Add("@utc", SqliteType.Text);

        foreach (var ev in events)
        {
            var utc = ev.Utc.ToString("O");
            fKey.Value = ev.PathKey;
            fKind.Value = (int)ev.Kind;
            fUtc.Value = utc;
            fWindow.Value = (ev.Utc - ChangeLogRules.CoalesceWindow).ToString("O");
            if (fold.ExecuteNonQuery() > 0) continue;

            iKey.Value = ev.PathKey;
            iDisplay.Value = ev.DisplayPath;
            iOld.Value = (object?)ev.OldDisplayPath ?? DBNull.Value;
            iIsDir.Value = ev.IsDirectory ? 1 : 0;
            iHidden.Value = ev.Hidden ? 1 : 0;
            iKind.Value = (int)ev.Kind;
            iUtc.Value = utc;
            insert.ExecuteNonQuery();
        }

        tx.Commit();
    }

    /// <summary>Drops rows older than <paramref name="retention"/>, then anything past the cap,
    /// newest kept. Idempotent, so two volumes pruning at once is redundant rather than wrong.</summary>
    public void Prune(DateTime nowUtc, TimeSpan retention, int maxRows = ChangeLogRules.MaxRows)
    {
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;

        cmd.CommandText = "DELETE FROM fs_change WHERE last_utc < @cutoff;";
        cmd.Parameters.AddWithValue("@cutoff", (nowUtc - retention).ToString("O"));
        cmd.ExecuteNonQuery();

        // The subselect is NULL while the table is under the cap, and a comparison with NULL
        // deletes nothing.
        cmd.CommandText =
            "DELETE FROM fs_change WHERE id < (SELECT id FROM fs_change ORDER BY id DESC LIMIT 1 OFFSET @keep);";
        cmd.Parameters.Clear();
        cmd.Parameters.AddWithValue("@keep", Math.Max(0, maxRows - 1));
        cmd.ExecuteNonQuery();

        tx.Commit();
    }

    /// <summary>The privacy switch: everything, gone.</summary>
    public void Clear()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM fs_change;";
        cmd.ExecuteNonQuery();
    }

    public long Count()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT count(*) FROM fs_change;";
        return (long)cmd.ExecuteScalar()!;
    }

    /// <summary>Changes whenever a row is added <em>or folded</em>; the live view polls this
    /// rather than re-running its query.</summary>
    public ChangeLogStamp Stamp()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT max(id), max(last_utc) FROM fs_change;";
        using var reader = cmd.ExecuteReader();
        reader.Read();
        return new ChangeLogStamp(
            reader.IsDBNull(0) ? 0 : reader.GetInt64(0),
            reader.IsDBNull(1) ? null : reader.GetString(1));
    }

    /// <summary>
    /// Newest first, bounded below by the range and above by the limit.
    /// </summary>
    /// <remarks>
    /// The time bound and the ordering both come off <c>ix_fs_change_last</c>, so the scan stops
    /// the moment the limit is full; scope, kind and hidden are filters on rows already in range,
    /// never a second index. <c>ChangeLogRepositoryTests.Query_NeedsNoSorter</c> holds the planner
    /// to that.
    /// </remarks>
    public (IReadOnlyList<ChangeRow> Rows, bool Truncated) Query(ChangeQuery query)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT id, path_key, display_path, old_path, is_dir, hidden, kind, first_utc, last_utc, count "
                          + Predicate(cmd, query)
                          + " ORDER BY last_utc DESC LIMIT @limit;";
        cmd.Parameters.AddWithValue("@limit", query.Limit + 1);

        var rows = new List<ChangeRow>();
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
        {
            if (rows.Count == query.Limit)
                return (rows, true);

            rows.Add(new ChangeRow(
                reader.GetInt64(0),
                reader.GetString(1),
                reader.GetString(2),
                reader.IsDBNull(3) ? null : reader.GetString(3),
                reader.GetInt64(4) != 0,
                reader.GetInt64(5) != 0,
                (ChangeKind)reader.GetInt64(6),
                ParseUtc(reader.GetString(7)),
                ParseUtc(reader.GetString(8)),
                (int)reader.GetInt64(9)));
        }
        return (rows, false);
    }

    /// <summary>The planner's account of <see cref="Query"/>, for the test that keeps it a range scan.</summary>
    internal IReadOnlyList<string> ExplainQuery(ChangeQuery query)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "EXPLAIN QUERY PLAN SELECT id " + Predicate(cmd, query) + " ORDER BY last_utc DESC LIMIT @limit;";
        cmd.Parameters.AddWithValue("@limit", query.Limit + 1);

        var lines = new List<string>();
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
            lines.Add(reader.GetString(reader.FieldCount - 1));
        return lines;
    }

    private static string Predicate(SqliteCommand cmd, ChangeQuery query)
    {
        // Pinned: given the scope bounds the planner prefers ix_fs_change_key and then sorts, which
        // materialises every row in range before the LIMIT can stop anything.
        var sql = " FROM fs_change INDEXED BY ix_fs_change_last WHERE last_utc >= @since";
        cmd.Parameters.AddWithValue("@since", query.SinceUtc.ToString("O"));

        if (query.ScopePathKey is { } scope)
        {
            var (lo, hi) = PathKey.PrefixBounds(scope);
            sql += " AND path_key >= @lo AND path_key < @hi";
            cmd.Parameters.AddWithValue("@lo", lo);
            cmd.Parameters.AddWithValue("@hi", hi);
        }

        // Fewer than all four: a literal list, since the values are this enum's own integers.
        if (query.Kinds.Count < 4)
            sql += query.Kinds.Count == 0
                ? " AND 0"
                : $" AND kind IN ({string.Join(",", query.Kinds.Select(k => (int)k).OrderBy(k => k))})";

        if (!query.IncludeHidden)
            sql += " AND hidden = 0";

        return sql;
    }

    private static DateTime ParseUtc(string text) =>
        DateTime.Parse(text, null, System.Globalization.DateTimeStyles.RoundtripKind);
}
