using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Core.Data;

/// <summary>
/// Persistence for the fs_entry / fs_index_root search index. Like the other
/// repositories this is synchronous ADO.NET with a pooled connection per method;
/// SearchService layers Task.Run on top.
/// </summary>
public sealed class FsIndexRepository
{
    private readonly Db _db;

    public FsIndexRepository(Db db) => _db = db;

    /// <summary>Bulk-upserts one chunk of crawled entries in a single transaction.</summary>
    public void UpsertEntries(IReadOnlyList<FsEntryRow> rows, long crawlGen)
    {
        if (rows.Count == 0) return;

        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        UpsertEntriesCore(conn, tx, rows, crawlGen);
        tx.Commit();
    }

    private static void UpsertEntriesCore(
        SqliteConnection conn, SqliteTransaction tx, IReadOnlyList<FsEntryRow> rows, long crawlGen)
    {
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;
        cmd.CommandText =
            """
            INSERT INTO fs_entry(path_key, name, name_key, is_dir, size_bytes, modified_utc, crawl_gen)
            VALUES (@key, @name, @nameKey, @isDir, @size, @modified, @gen)
            ON CONFLICT(path_key) DO UPDATE SET
                name = excluded.name,
                name_key = excluded.name_key,
                is_dir = excluded.is_dir,
                size_bytes = excluded.size_bytes,
                modified_utc = excluded.modified_utc,
                crawl_gen = excluded.crawl_gen;
            """;
        var pKey = cmd.Parameters.Add("@key", SqliteType.Text);
        var pName = cmd.Parameters.Add("@name", SqliteType.Text);
        var pNameKey = cmd.Parameters.Add("@nameKey", SqliteType.Text);
        var pIsDir = cmd.Parameters.Add("@isDir", SqliteType.Integer);
        var pSize = cmd.Parameters.Add("@size", SqliteType.Integer);
        var pModified = cmd.Parameters.Add("@modified", SqliteType.Text);
        var pGen = cmd.Parameters.Add("@gen", SqliteType.Integer);

        foreach (var row in rows)
        {
            pKey.Value = row.PathKey;
            pName.Value = row.Name;
            pNameKey.Value = row.Name.ToUpperInvariant();
            pIsDir.Value = row.IsDirectory ? 1 : 0;
            pSize.Value = row.SizeBytes;
            pModified.Value = row.ModifiedUtc.ToString("O");
            pGen.Value = crawlGen;
            cmd.ExecuteNonQuery();
        }
    }

    /// <summary>
    /// Removes entries under <paramref name="rootKey"/> that a completed crawl did not
    /// touch (their crawl_gen predates the crawl) — i.e. entries that vanished from disk.
    /// </summary>
    public void SweepVanished(string rootKey, long crawlGen)
    {
        var (lo, hi) = PathKey.PrefixBounds(rootKey);
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            "DELETE FROM fs_entry WHERE path_key >= @lo AND path_key < @hi AND crawl_gen < @gen;";
        cmd.Parameters.AddWithValue("@lo", lo);
        cmd.Parameters.AddWithValue("@hi", hi);
        cmd.Parameters.AddWithValue("@gen", crawlGen);
        cmd.ExecuteNonQuery();
    }

    /// <summary>Registers (or refreshes) an indexed root; always clears the stale flag.</summary>
    public void UpsertRoot(string rootKey, string displayPath, DateTime crawledUtc, bool complete)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            """
            INSERT INTO fs_index_root(path_key, display_path, crawled_utc, complete, stale)
            VALUES (@key, @display, @crawled, @complete, 0)
            ON CONFLICT(path_key) DO UPDATE SET
                display_path = excluded.display_path,
                crawled_utc = excluded.crawled_utc,
                complete = excluded.complete,
                stale = 0;
            """;
        cmd.Parameters.AddWithValue("@key", rootKey);
        cmd.Parameters.AddWithValue("@display", displayPath);
        cmd.Parameters.AddWithValue("@crawled", crawledUtc.ToString("O"));
        cmd.Parameters.AddWithValue("@complete", complete ? 1 : 0);
        cmd.ExecuteNonQuery();
    }

    /// <summary>Flags an indexed root as needing a re-crawl (e.g. watcher buffer overflow).</summary>
    public void MarkRootStale(string rootKey)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "UPDATE fs_index_root SET stale = 1 WHERE path_key = @key;";
        cmd.Parameters.AddWithValue("@key", rootKey);
        cmd.ExecuteNonQuery();
    }

    /// <summary>
    /// Finds the completely-indexed root that covers <paramref name="path"/>
    /// (ancestor-or-equal), preferring non-stale, then deepest. Null if uncovered.
    /// </summary>
    public FsIndexRoot? FindCoveringRoot(string path)
    {
        var chain = new List<string>();
        for (var k = PathKey.Canonicalize(path); k is not null; k = Path.GetDirectoryName(k))
            chain.Add(k);

        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        var parms = string.Join(", ", Enumerable.Range(0, chain.Count).Select(i => $"@p{i}"));
        cmd.CommandText =
            $"""
            SELECT path_key, display_path, crawled_utc, complete, stale
            FROM fs_index_root
            WHERE complete = 1 AND path_key IN ({parms});
            """;
        for (var i = 0; i < chain.Count; i++)
            cmd.Parameters.AddWithValue($"@p{i}", chain[i]);

        var candidates = new List<FsIndexRoot>();
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
        {
            candidates.Add(new FsIndexRoot(
                reader.GetString(0),
                reader.GetString(1),
                DateTime.Parse(reader.GetString(2), null, System.Globalization.DateTimeStyles.RoundtripKind),
                reader.GetInt32(3) != 0,
                reader.GetInt32(4) != 0));
        }

        return candidates
            .OrderBy(r => r.Stale)                    // fresh roots first
            .ThenByDescending(r => r.PathKey.Length)  // then deepest
            .FirstOrDefault();
    }

    /// <summary>
    /// Range-scans the subtree under <paramref name="rootPath"/> applying every query
    /// term as a GLOB on name_key, capped at <paramref name="cap"/> hits. No ORDER BY:
    /// LIMIT lets SQLite stop scanning as soon as enough matches are found; callers
    /// sort the small result page. Relative display paths are reconstructed from the
    /// ancestor directory rows (full display paths are not stored — they would roughly
    /// double the index size).
    /// </summary>
    public (IReadOnlyList<SearchHit> Hits, bool Truncated) Search(string rootPath, SearchQuery query, int cap)
    {
        var rootKey = PathKey.Canonicalize(rootPath);
        var rootDisplay = PathKey.NormalizeDisplay(rootPath);
        var (lo, hi) = PathKey.PrefixBounds(rootKey);

        using var conn = _db.Open();

        var rows = new List<(string Key, string Name, bool IsDir, long Size, DateTime Modified)>();
        using (var cmd = conn.CreateCommand())
        {
            var globs = string.Join(" AND ", Enumerable.Range(0, query.GlobPatterns.Count)
                .Select(i => $"name_key GLOB @g{i}"));
            cmd.CommandText =
                $"""
                SELECT path_key, name, is_dir, size_bytes, modified_utc
                FROM fs_entry
                WHERE path_key >= @lo AND path_key < @hi AND {globs}
                LIMIT @limit;
                """;
            cmd.Parameters.AddWithValue("@lo", lo);
            cmd.Parameters.AddWithValue("@hi", hi);
            cmd.Parameters.AddWithValue("@limit", cap + 1);
            for (var i = 0; i < query.GlobPatterns.Count; i++)
                cmd.Parameters.AddWithValue($"@g{i}", query.GlobPatterns[i]);

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
            {
                rows.Add((
                    reader.GetString(0),
                    reader.GetString(1),
                    reader.GetInt32(2) != 0,
                    reader.GetInt64(3),
                    DateTime.Parse(reader.GetString(4), null, System.Globalization.DateTimeStyles.RoundtripKind)));
            }
        }

        var truncated = rows.Count > cap;
        if (truncated)
            rows.RemoveAt(rows.Count - 1);

        var ancestorNames = LookupAncestorNames(conn, rows, lo.Length);

        var hits = new List<SearchHit>(rows.Count);
        foreach (var row in rows)
        {
            var relDir = BuildRelativeDir(row.Key, lo.Length, ancestorNames);
            hits.Add(new SearchHit(
                Path.Combine(rootDisplay, relDir, row.Name),
                relDir,
                row.Name,
                row.IsDir,
                row.Size,
                row.Modified));
        }
        return (hits, truncated);
    }

    /// <summary>
    /// Display names for every distinct ancestor directory (strictly between the search
    /// root and each hit). Each ancestor is itself an fs_entry row keyed by a prefix of
    /// the hit's path_key.
    /// </summary>
    private static Dictionary<string, string> LookupAncestorNames(
        SqliteConnection conn,
        List<(string Key, string Name, bool IsDir, long Size, DateTime Modified)> rows,
        int loLength)
    {
        var keys = new HashSet<string>(StringComparer.Ordinal);
        foreach (var row in rows)
        {
            for (var i = row.Key.IndexOf('\\', loLength); i >= 0; i = row.Key.IndexOf('\\', i + 1))
                keys.Add(row.Key[..i]);
        }

        var names = new Dictionary<string, string>(keys.Count, StringComparer.Ordinal);
        foreach (var chunk in keys.Chunk(500))
        {
            using var cmd = conn.CreateCommand();
            var parms = string.Join(", ", Enumerable.Range(0, chunk.Length).Select(i => $"@p{i}"));
            cmd.CommandText = $"SELECT path_key, name FROM fs_entry WHERE path_key IN ({parms});";
            for (var i = 0; i < chunk.Length; i++)
                cmd.Parameters.AddWithValue($"@p{i}", chunk[i]);

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
                names[reader.GetString(0)] = reader.GetString(1);
        }
        return names;
    }

    private static string BuildRelativeDir(string pathKey, int loLength, Dictionary<string, string> ancestorNames)
    {
        var lastSep = pathKey.LastIndexOf('\\');
        if (lastSep < loLength)
            return ""; // direct child of the search root

        var segments = new List<string>();
        for (var i = pathKey.IndexOf('\\', loLength); i >= 0; i = pathKey.IndexOf('\\', i + 1))
        {
            var ancestorKey = pathKey[..i];
            // Fallback to the uppercase key segment if the ancestor row is missing.
            segments.Add(ancestorNames.TryGetValue(ancestorKey, out var name)
                ? name
                : ancestorKey[(ancestorKey.LastIndexOf('\\') + 1)..]);
        }
        return string.Join('\\', segments);
    }

    /// <summary>Watcher apply: removes an entry and (for directories) its whole subtree.</summary>
    public void DeleteSubtree(string pathKey)
    {
        var (lo, hi) = PathKey.PrefixBounds(pathKey);
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;
        cmd.CommandText = "DELETE FROM fs_entry WHERE path_key = @key;";
        cmd.Parameters.AddWithValue("@key", pathKey);
        cmd.ExecuteNonQuery();

        cmd.CommandText = "DELETE FROM fs_entry WHERE path_key >= @lo AND path_key < @hi;";
        cmd.Parameters.Clear();
        cmd.Parameters.AddWithValue("@lo", lo);
        cmd.Parameters.AddWithValue("@hi", hi);
        cmd.ExecuteNonQuery();
        tx.Commit();
    }

    /// <summary>
    /// Watcher apply: renames an entry, rewriting descendant keys in place — the keys
    /// embed the path, so a prefix rewrite avoids any re-crawl. Whatever previously
    /// existed at the target is removed first (overwrite-moves).
    /// </summary>
    public void Rename(string oldPathKey, string newPathKey, string newName, long crawlGen)
    {
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;

        if (!string.Equals(oldPathKey, newPathKey, StringComparison.Ordinal))
        {
            var (oldLo, oldHi) = PathKey.PrefixBounds(oldPathKey);
            var (newLo, newHi) = PathKey.PrefixBounds(newPathKey);

            cmd.CommandText = "DELETE FROM fs_entry WHERE path_key = @new;";
            cmd.Parameters.AddWithValue("@new", newPathKey);
            cmd.ExecuteNonQuery();

            cmd.CommandText = "DELETE FROM fs_entry WHERE path_key >= @newLo AND path_key < @newHi;";
            cmd.Parameters.Clear();
            cmd.Parameters.AddWithValue("@newLo", newLo);
            cmd.Parameters.AddWithValue("@newHi", newHi);
            cmd.ExecuteNonQuery();

            // length(@old) is evaluated by SQLite so character counting matches substr's.
            cmd.CommandText =
                """
                UPDATE fs_entry
                SET path_key = @new || substr(path_key, length(@old) + 1), crawl_gen = @gen
                WHERE path_key >= @oldLo AND path_key < @oldHi;
                """;
            cmd.Parameters.Clear();
            cmd.Parameters.AddWithValue("@new", newPathKey);
            cmd.Parameters.AddWithValue("@old", oldPathKey);
            cmd.Parameters.AddWithValue("@gen", crawlGen);
            cmd.Parameters.AddWithValue("@oldLo", oldLo);
            cmd.Parameters.AddWithValue("@oldHi", oldHi);
            cmd.ExecuteNonQuery();

            cmd.CommandText =
                """
                UPDATE fs_entry
                SET path_key = @new, name = @name, name_key = @nameKey, crawl_gen = @gen
                WHERE path_key = @old;
                """;
            cmd.Parameters.Clear();
            cmd.Parameters.AddWithValue("@new", newPathKey);
            cmd.Parameters.AddWithValue("@name", newName);
            cmd.Parameters.AddWithValue("@nameKey", newName.ToUpperInvariant());
            cmd.Parameters.AddWithValue("@gen", crawlGen);
            cmd.Parameters.AddWithValue("@old", oldPathKey);
            cmd.ExecuteNonQuery();
        }
        else
        {
            // Case-only rename: the key is unchanged, only the display name moves.
            cmd.CommandText =
                """
                UPDATE fs_entry
                SET name = @name, name_key = @nameKey, crawl_gen = @gen
                WHERE path_key = @key;
                """;
            cmd.Parameters.AddWithValue("@name", newName);
            cmd.Parameters.AddWithValue("@nameKey", newName.ToUpperInvariant());
            cmd.Parameters.AddWithValue("@gen", crawlGen);
            cmd.Parameters.AddWithValue("@key", oldPathKey);
            cmd.ExecuteNonQuery();
        }

        tx.Commit();
    }
}
