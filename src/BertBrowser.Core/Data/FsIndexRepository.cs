using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.Search;
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
            INSERT INTO fs_entry(path_key, name, name_key, is_dir, size_bytes, modified_utc, hidden, crawl_gen)
            VALUES (@key, @name, @nameKey, @isDir, @size, @modified, @hidden, @gen)
            ON CONFLICT(path_key) DO UPDATE SET
                name = excluded.name,
                name_key = excluded.name_key,
                is_dir = excluded.is_dir,
                size_bytes = excluded.size_bytes,
                modified_utc = excluded.modified_utc,
                hidden = excluded.hidden,
                crawl_gen = excluded.crawl_gen;
            """;
        var pKey = cmd.Parameters.Add("@key", SqliteType.Text);
        var pName = cmd.Parameters.Add("@name", SqliteType.Text);
        var pNameKey = cmd.Parameters.Add("@nameKey", SqliteType.Text);
        var pIsDir = cmd.Parameters.Add("@isDir", SqliteType.Integer);
        var pSize = cmd.Parameters.Add("@size", SqliteType.Integer);
        var pModified = cmd.Parameters.Add("@modified", SqliteType.Text);
        var pHidden = cmd.Parameters.Add("@hidden", SqliteType.Integer);
        var pGen = cmd.Parameters.Add("@gen", SqliteType.Integer);

        foreach (var row in rows)
        {
            pKey.Value = row.PathKey;
            pName.Value = row.Name;
            pNameKey.Value = row.Name.ToUpperInvariant();
            pIsDir.Value = row.IsDirectory ? 1 : 0;
            pSize.Value = row.SizeBytes;
            pModified.Value = row.ModifiedUtc.ToString("O");
            pHidden.Value = row.Hidden ? 1 : 0;
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
    /// Range-scans the subtree under <paramref name="rootPath"/> applying the query, capped at
    /// <paramref name="cap"/> hits. No ORDER BY: LIMIT lets SQLite stop scanning as soon as
    /// enough matches are found; callers sort the small result page. Relative display paths are
    /// reconstructed from the ancestor directory rows (full display paths are not stored — they
    /// would roughly double the index size).
    /// </summary>
    /// <remarks>
    /// The compiled predicate is a filter on a range scan, not a seek: there is no index on
    /// size_bytes or modified_utc, and deliberately so — fs_entry is WITHOUT ROWID, so a
    /// secondary index would carry the whole path_key as its row reference, adding hundreds of
    /// megabytes and a second B-tree to write on every upsert chunk of a build that runs at
    /// every launch. That is the same trade 002_search_index.sql made against indexing name, and
    /// the one DuplicateCandidates measured at 0.8 s for a full scan of 1.09 M rows. A selective
    /// filter therefore costs a longer scan before the cap fills, never a different plan.
    /// </remarks>
    public (IReadOnlyList<SearchHit> Hits, bool Truncated) Search(
        string rootPath, SearchQuery query, int cap, bool includeHidden = true)
    {
        var rootKey = PathKey.Canonicalize(rootPath);
        var rootDisplay = PathKey.NormalizeDisplay(rootPath);
        var (lo, hi) = PathKey.PrefixBounds(rootKey);

        using var conn = _db.Open();

        var predicate = query.Compile();
        var rows = new List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)>();
        bool truncated;
        using (var cmd = conn.CreateCommand())
        {
            var hiddenFilter = includeHidden ? "" : "AND hidden = 0 ";
            cmd.CommandText =
                $"""
                SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, name_key
                FROM fs_entry
                WHERE path_key >= @lo AND path_key < @hi AND ({predicate.Sql}) {hiddenFilter}
                {Limit(predicate)};
                """;
            cmd.Parameters.AddWithValue("@lo", lo);
            cmd.Parameters.AddWithValue("@hi", hi);
            cmd.Parameters.AddWithValue("@limit", cap + 1);
            Bind(cmd, predicate);

            truncated = ReadMatchingRows(cmd, rows, query, cap);
        }

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
                row.Modified,
                row.Hidden));
        }
        return (hits, truncated);
    }

    /// <summary>
    /// Whether anything in scope carries a real byte length — <c>null</c> for the whole index.
    /// </summary>
    /// <remarks>
    /// <para>The <c>FSCTL_ENUM_USN_DATA</c> fallback build records names only: every row lands
    /// with <c>size_bytes = 0</c> and no timestamp. A size or date filter over such a volume
    /// therefore matches nothing however the disk actually looks, and "no results" would be a
    /// lie — the same trap <c>DiskUsageRules</c> exists to keep out of the disk-usage view,
    /// recognised the same way: by there being rows but not one length among them.</para>
    /// <para>Asked only when a metadata-filtered search came back empty, which is both rare and
    /// already the slow path. On a measured volume it stops at the first row; on an unmeasured
    /// one it scans, and that is the one case where the answer is worth the scan.</para>
    /// </remarks>
    public bool HasSizeData(string? rootPath)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();

        if (rootPath is null)
        {
            cmd.CommandText = "SELECT EXISTS(SELECT 1 FROM fs_entry WHERE size_bytes > 0);";
        }
        else
        {
            var (lo, hi) = PathKey.PrefixBounds(PathKey.Canonicalize(rootPath));
            cmd.CommandText =
                """
                SELECT EXISTS(
                    SELECT 1 FROM fs_entry
                    WHERE path_key >= @lo AND path_key < @hi AND size_bytes > 0);
                """;
            cmd.Parameters.AddWithValue("@lo", lo);
            cmd.Parameters.AddWithValue("@hi", hi);
        }

        return Convert.ToInt64(cmd.ExecuteScalar()) != 0;
    }

    /// <summary>The drive root prefix length in a canonical key: "C:\" — MFT-indexed roots
    /// are always local NTFS drives, so every global hit shares this 3-char root.</summary>
    private const int DriveRootLength = 3;

    /// <summary>
    /// Whole-index ("This PC") search: the same predicate as <see cref="Search"/> but with
    /// no subtree bound, so it scans every indexed volume. Results carry full display paths
    /// (Everything-style) reconstructed from each row's ancestor directory names up to its
    /// drive root. <see cref="SearchHit.RelativeDirDisplay"/> holds the full parent path.
    /// </summary>
    public (IReadOnlyList<SearchHit> Hits, bool Truncated) SearchGlobal(
        SearchQuery query, int cap, bool includeHidden = true)
    {
        using var conn = _db.Open();

        var predicate = query.Compile();
        var rows = new List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)>();
        bool truncated;
        using (var cmd = conn.CreateCommand())
        {
            var hiddenFilter = includeHidden ? "" : "AND hidden = 0 ";
            cmd.CommandText =
                $"""
                SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, name_key
                FROM fs_entry
                WHERE ({predicate.Sql}) {hiddenFilter}
                {Limit(predicate)};
                """;
            cmd.Parameters.AddWithValue("@limit", cap + 1);
            Bind(cmd, predicate);

            truncated = ReadMatchingRows(cmd, rows, query, cap);
        }

        if (truncated)
            rows.RemoveAt(rows.Count - 1);

        var ancestorNames = LookupAncestorNames(conn, rows, DriveRootLength);

        var hits = new List<SearchHit>(rows.Count);
        foreach (var row in rows)
        {
            var driveRoot = row.Key[..DriveRootLength]; // "C:\"
            var relDir = BuildRelativeDir(row.Key, DriveRootLength, ancestorNames);
            var parentFull = relDir.Length == 0 ? driveRoot : driveRoot + relDir; // driveRoot ends with '\'
            var display = parentFull.EndsWith('\\') ? parentFull + row.Name : parentFull + '\\' + row.Name;
            hits.Add(new SearchHit(display, parentFull, row.Name, row.IsDir, row.Size, row.Modified, row.Hidden));
        }
        return (hits, truncated);
    }

    /// <summary>
    /// The LIMIT clause for a compiled predicate — and only when it is exact.
    /// </summary>
    /// <remarks>
    /// An incomplete predicate is a <em>superset</em> of the query (a regex term compiles to
    /// <c>1</c>), so the rows SQLite returns are filtered again in C#. Letting it stop at
    /// <c>cap + 1</c> rows would then cap the wrong population: the first thousand rows the scan
    /// happened to reach, most of which the re-check throws away. The cap is applied by
    /// <see cref="ReadMatchingRows"/> instead, which counts rows that actually matched.
    /// </remarks>
    private static string Limit(SqlPredicate predicate) => predicate.Complete ? "LIMIT @limit" : "";

    /// <summary>Binds the values a compiled predicate carries.</summary>
    private static void Bind(SqliteCommand cmd, SqlPredicate predicate)
    {
        foreach (var (name, value) in predicate.Parameters)
            cmd.Parameters.AddWithValue(name, value);
    }

    /// <summary>
    /// Reads rows, keeping only those the query really matches, and stops one past
    /// <paramref name="cap"/>. Returns whether that extra row was reached.
    /// </summary>
    /// <remarks>
    /// <strong>This re-check is what makes the SQL an optimisation rather than a second
    /// implementation.</strong> <c>SearchNode.Matches</c> is the definition of a hit; a
    /// <c>WriteSql</c> that is too wide costs a longer scan and nothing else, because the extra
    /// rows die here. Delete this and a regex search returns every row in the subtree.
    /// </remarks>
    private static bool ReadMatchingRows(
        SqliteCommand cmd,
        List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)> rows,
        SearchQuery query,
        int cap)
    {
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
        {
            var key = reader.GetString(0);
            var name = reader.GetString(1);
            var isDir = reader.GetInt32(2) != 0;
            var size = reader.GetInt64(3);
            var modified = DateTime.Parse(
                reader.GetString(4), null, System.Globalization.DateTimeStyles.RoundtripKind);
            var hidden = reader.GetInt32(5) != 0;
            var nameKey = reader.GetString(6);

            if (!query.Matches(new SearchCandidate(nameKey, key, isDir, size, modified, hidden)))
                continue;

            rows.Add((key, name, isDir, size, modified, hidden));
            if (rows.Count > cap)
                return true;
        }
        return false;
    }

    /// <summary>Reads the six-column entry projection every query in this file selects, in the
    /// order they all declare it.</summary>
    private static void ReadRows(
        SqliteCommand cmd,
        List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)> rows)
    {
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
        {
            rows.Add((
                reader.GetString(0),
                reader.GetString(1),
                reader.GetInt32(2) != 0,
                reader.GetInt64(3),
                DateTime.Parse(reader.GetString(4), null, System.Globalization.DateTimeStyles.RoundtripKind),
                reader.GetInt32(5) != 0));
        }
    }

    /// <summary>
    /// The biggest files under <paramref name="rootPath"/> — or across every indexed volume when
    /// it is null — largest first.
    /// </summary>
    /// <remarks>
    /// Unlike <see cref="Search"/> this <em>does</em> order, because "the largest files" means
    /// nothing otherwise. So LIMIT no longer lets the scan stop early; it bounds SQLite's sorter
    /// to N rows instead of the result set. A scoped call costs one range scan of that subtree —
    /// the access pattern the clustered key is built for. An unscoped one is a full scan of the
    /// index and takes seconds on a large disk, which is why this is only ever reached from an
    /// explicit "go and compute this" screen rather than from a keystroke.
    /// <para>
    /// Directories are excluded by the query rather than by trusting them to be zero: a folder's
    /// total lives in dir_size_cache, and a row here is only ever a file's own bytes.
    /// </para>
    /// </remarks>
    public IReadOnlyList<SearchHit> LargestFiles(string? rootPath, int limit, bool includeHidden = true)
    {
        if (limit <= 0) return [];

        var scoped = rootPath is { Length: > 0 };
        var rootKey = scoped ? PathKey.Canonicalize(rootPath!) : "";
        var rootDisplay = scoped ? PathKey.NormalizeDisplay(rootPath!) : "";
        var (lo, hi) = scoped ? PathKey.PrefixBounds(rootKey) : ("", "");

        using var conn = _db.Open();

        var rows = new List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)>();
        using (var cmd = conn.CreateCommand())
        {
            var scope = scoped ? "path_key >= @lo AND path_key < @hi AND " : "";
            var hiddenFilter = includeHidden ? "" : "AND hidden = 0 ";
            cmd.CommandText =
                $"""
                SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden
                FROM fs_entry
                WHERE {scope}is_dir = 0 {hiddenFilter}
                ORDER BY size_bytes DESC
                LIMIT @limit;
                """;
            if (scoped)
            {
                cmd.Parameters.AddWithValue("@lo", lo);
                cmd.Parameters.AddWithValue("@hi", hi);
            }
            cmd.Parameters.AddWithValue("@limit", limit);

            ReadRows(cmd, rows);
        }

        // Same reconstruction the two search paths do: a scoped result is relative to the root it
        // was asked about, an unscoped one carries the full path from its drive root.
        return BuildHits(conn, rows, scoped, rootDisplay, lo);
    }

    /// <summary>
    /// Display names for every distinct ancestor directory (strictly between the search
    /// root and each hit). Each ancestor is itself an fs_entry row keyed by a prefix of
    /// the hit's path_key.
    /// </summary>
    private static Dictionary<string, string> LookupAncestorNames(
        SqliteConnection conn,
        List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)> rows,
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

    /// <summary>
    /// Every file at or above <paramref name="minSizeBytes"/> that shares its byte length with at
    /// least one other — the shortlist a duplicate scan starts from, with no file opened.
    /// </summary>
    /// <param name="exclude">
    /// Given a canonical path key, true to drop the row. Applied in <em>both</em> passes so an
    /// excluded file cannot prop up a size group that then turns out to have one member.
    /// </param>
    /// <remarks>
    /// <para>
    /// <b>Two streaming scans, and deliberately no <c>GROUP BY</c>.</b> Grouping or ordering by
    /// <c>size_bytes</c> would make SQLite materialise a temp B-tree over every qualifying
    /// (size, path_key) pair — hundreds of megabytes on a real index, since there is no index on
    /// size and the pair is most of the row. Two ordered walks of the clustered table need no
    /// sorter at all: the first counts sizes into a dictionary bounded by the number of
    /// <em>distinct</em> sizes, the second collects only the rows whose size was seen twice. The
    /// second walk reads the pages the first one just warmed.
    /// </para>
    /// <para>
    /// <b>An index on <c>size_bytes</c> was considered and refused.</b> <c>fs_entry</c> is
    /// <c>WITHOUT ROWID</c>, so a secondary index carries the whole <c>path_key</c> as its row
    /// reference — several hundred megabytes added to the database, and a second B-tree to write on
    /// every upsert chunk of a build that runs at every launch. This is the same trade
    /// <c>002_search_index.sql</c> already made against an index on <c>name</c>.
    /// </para>
    /// <para>
    /// <b>The first pass counts two things it does not need</b> — files in scope, and files in scope
    /// with a real length — because they are what tells a volume indexed by the sizeless
    /// <c>FSCTL_ENUM_USN_DATA</c> path apart from one that genuinely holds no duplicates. Both come
    /// free from a walk that is happening anyway, and asking separately would mean a third scan.
    /// They are counted <em>before</em> <paramref name="exclude"/> runs: they describe what the
    /// index knows, not what this caller asked to see.
    /// </para>
    /// <para>
    /// Like <see cref="LargestFiles"/> this is only ever reached from an explicit "go and compute
    /// this" gesture, never from a keystroke.
    /// </para>
    /// </remarks>
    public DuplicateShortlist DuplicateCandidates(
        string? rootPath,
        long minSizeBytes,
        bool includeHidden,
        Func<string, bool>? exclude = null,
        CancellationToken ct = default)
    {
        // Zero would sweep in every file the sizeless build path could not measure, and compare
        // them all against each other. One byte is the smallest honest floor.
        var floor = Math.Max(1, minSizeBytes);

        var scoped = rootPath is { Length: > 0 };
        var rootKey = scoped ? PathKey.Canonicalize(rootPath!) : "";
        var rootDisplay = scoped ? PathKey.NormalizeDisplay(rootPath!) : "";
        var (lo, hi) = scoped ? PathKey.PrefixBounds(rootKey) : ("", "");

        var scope = scoped ? "path_key >= @lo AND path_key < @hi AND " : "";
        var hiddenFilter = includeHidden ? "" : "AND hidden = 0 ";

        using var conn = _db.Open();

        // --- pass one: which byte lengths occur more than once ---
        var counts = new Dictionary<long, int>();
        var filesInScope = 0;
        var sizedFilesInScope = 0;

        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText =
                $"""
                SELECT size_bytes, path_key
                FROM fs_entry
                WHERE {scope}is_dir = 0 {hiddenFilter};
                """;
            if (scoped)
            {
                cmd.Parameters.AddWithValue("@lo", lo);
                cmd.Parameters.AddWithValue("@hi", hi);
            }

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
            {
                ct.ThrowIfCancellationRequested();

                var size = reader.GetInt64(0);
                filesInScope++;
                if (size > 0) sizedFilesInScope++;

                if (size < floor) continue;
                if (exclude is not null && exclude(reader.GetString(1))) continue;

                counts[size] = counts.TryGetValue(size, out var seen) ? seen + 1 : 1;
            }
        }

        var colliding = new HashSet<long>();
        foreach (var (size, seen) in counts)
            if (seen > 1) colliding.Add(size);

        if (colliding.Count == 0)
            return new DuplicateShortlist([], filesInScope, sizedFilesInScope);

        // --- pass two: the rows at those lengths ---
        var rows = new List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)>();
        using (var cmd = conn.CreateCommand())
        {
            cmd.CommandText =
                $"""
                SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden
                FROM fs_entry
                WHERE {scope}is_dir = 0 AND size_bytes >= @min {hiddenFilter};
                """;
            if (scoped)
            {
                cmd.Parameters.AddWithValue("@lo", lo);
                cmd.Parameters.AddWithValue("@hi", hi);
            }
            cmd.Parameters.AddWithValue("@min", floor);

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
            {
                ct.ThrowIfCancellationRequested();

                var size = reader.GetInt64(3);
                if (!colliding.Contains(size)) continue;

                var key = reader.GetString(0);
                if (exclude is not null && exclude(key)) continue;

                rows.Add((
                    key,
                    reader.GetString(1),
                    reader.GetInt32(2) != 0,
                    size,
                    DateTime.Parse(reader.GetString(4), null, System.Globalization.DateTimeStyles.RoundtripKind),
                    reader.GetInt32(5) != 0));
            }
        }

        return new DuplicateShortlist(
            BuildHits(conn, rows, scoped, rootDisplay, lo), filesInScope, sizedFilesInScope);
    }

    /// <summary>
    /// Turns raw rows into hits with display paths rebuilt from their ancestors' names — the
    /// reconstruction <see cref="LargestFiles"/> and the duplicate shortlist both need, since full
    /// display paths are not stored.
    /// </summary>
    private static IReadOnlyList<SearchHit> BuildHits(
        SqliteConnection conn,
        List<(string Key, string Name, bool IsDir, long Size, DateTime Modified, bool Hidden)> rows,
        bool scoped,
        string rootDisplay,
        string lo)
    {
        if (rows.Count == 0) return [];

        var prefixLength = scoped ? lo.Length : DriveRootLength;
        var ancestorNames = LookupAncestorNames(conn, rows, prefixLength);

        var hits = new List<SearchHit>(rows.Count);
        foreach (var row in rows)
        {
            var relDir = BuildRelativeDir(row.Key, prefixLength, ancestorNames);
            if (scoped)
            {
                hits.Add(new SearchHit(
                    Path.Combine(rootDisplay, relDir, row.Name),
                    relDir, row.Name, row.IsDir, row.Size, row.Modified, row.Hidden));
            }
            else
            {
                var driveRoot = row.Key[..DriveRootLength]; // "C:\"
                var parentFull = relDir.Length == 0 ? driveRoot : driveRoot + relDir;
                var display = parentFull.EndsWith('\\') ? parentFull + row.Name : parentFull + '\\' + row.Name;
                hits.Add(new SearchHit(
                    display, parentFull, row.Name, row.IsDir, row.Size, row.Modified, row.Hidden));
            }
        }
        return hits;
    }
}
