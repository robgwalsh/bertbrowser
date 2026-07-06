using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Core.Data;

public enum TagMatchMode
{
    Any,
    All,
}

public sealed class TagRepository
{
    private readonly Db _db;

    public TagRepository(Db db) => _db = db;

    public Tag CreateTag(string name, string? color)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            "INSERT INTO tag(name, color, created_utc) VALUES (@name, @color, @now) RETURNING id;";
        cmd.Parameters.AddWithValue("@name", name);
        cmd.Parameters.AddWithValue("@color", (object?)color ?? DBNull.Value);
        cmd.Parameters.AddWithValue("@now", DateTime.UtcNow.ToString("O"));
        var id = (long)cmd.ExecuteScalar()!;
        return new Tag(id, name, color);
    }

    public void RenameTag(long tagId, string newName)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "UPDATE tag SET name = @name WHERE id = @id;";
        cmd.Parameters.AddWithValue("@name", newName);
        cmd.Parameters.AddWithValue("@id", tagId);
        cmd.ExecuteNonQuery();
    }

    public void SetTagColor(long tagId, string? color)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "UPDATE tag SET color = @color WHERE id = @id;";
        cmd.Parameters.AddWithValue("@color", (object?)color ?? DBNull.Value);
        cmd.Parameters.AddWithValue("@id", tagId);
        cmd.ExecuteNonQuery();
    }

    public void DeleteTag(long tagId)
    {
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;
        cmd.CommandText = "DELETE FROM tag WHERE id = @id;";
        cmd.Parameters.AddWithValue("@id", tagId);
        cmd.ExecuteNonQuery();
        DeleteOrphanFiles(cmd);
        tx.Commit();
    }

    public IReadOnlyList<Tag> GetAllTags()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT id, name, color FROM tag ORDER BY name COLLATE NOCASE;";
        using var reader = cmd.ExecuteReader();
        var tags = new List<Tag>();
        while (reader.Read())
            tags.Add(new Tag(reader.GetInt64(0), reader.GetString(1), reader.IsDBNull(2) ? null : reader.GetString(2)));
        return tags;
    }

    public int GetTagUsageCount(long tagId)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT COUNT(*) FROM file_tag WHERE tag_id = @id;";
        cmd.Parameters.AddWithValue("@id", tagId);
        return Convert.ToInt32(cmd.ExecuteScalar());
    }

    public void AssignTags(IEnumerable<string> paths, IReadOnlyCollection<long> tagIds)
    {
        if (tagIds.Count == 0) return;

        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();

        using var upsertFile = conn.CreateCommand();
        upsertFile.Transaction = tx;
        upsertFile.CommandText =
            """
            INSERT INTO file(path_key, display_path) VALUES (@key, @display)
            ON CONFLICT(path_key) DO UPDATE SET display_path = excluded.display_path
            RETURNING id;
            """;
        var pKey = upsertFile.Parameters.Add("@key", SqliteType.Text);
        var pDisplay = upsertFile.Parameters.Add("@display", SqliteType.Text);

        using var insertLink = conn.CreateCommand();
        insertLink.Transaction = tx;
        insertLink.CommandText =
            "INSERT OR IGNORE INTO file_tag(file_id, tag_id) VALUES (@file, @tag);";
        var pFile = insertLink.Parameters.Add("@file", SqliteType.Integer);
        var pTag = insertLink.Parameters.Add("@tag", SqliteType.Integer);

        foreach (var path in paths)
        {
            pKey.Value = PathKey.Canonicalize(path);
            pDisplay.Value = PathKey.NormalizeDisplay(path);
            var fileId = (long)upsertFile.ExecuteScalar()!;
            pFile.Value = fileId;
            foreach (var tagId in tagIds)
            {
                pTag.Value = tagId;
                insertLink.ExecuteNonQuery();
            }
        }

        tx.Commit();
    }

    public void UnassignTags(IEnumerable<string> paths, IReadOnlyCollection<long> tagIds)
    {
        if (tagIds.Count == 0) return;

        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();

        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;
        cmd.CommandText =
            $"""
            DELETE FROM file_tag
            WHERE file_id = (SELECT id FROM file WHERE path_key = @key)
              AND tag_id IN ({ParamList("t", tagIds.Count)});
            """;
        var pKey = cmd.Parameters.Add("@key", SqliteType.Text);
        var i = 0;
        foreach (var tagId in tagIds)
            cmd.Parameters.AddWithValue($"@t{i++}", tagId);

        foreach (var path in paths)
        {
            pKey.Value = PathKey.Canonicalize(path);
            cmd.ExecuteNonQuery();
        }

        DeleteOrphanFiles(cmd);
        tx.Commit();
    }

    /// <summary>
    /// Repoints file rows after a move so tags follow the entry: the exact path (a moved
    /// file, or the directory row itself) plus — when a directory moved — every key under
    /// the old prefix. Rare conflicts with an existing row at the target key are skipped
    /// (OR IGNORE) rather than clobbering that row's tags.
    /// </summary>
    public void UpdateEntryPaths(string oldPath, string newPath)
    {
        var oldKey = PathKey.Canonicalize(oldPath);
        var newKey = PathKey.Canonicalize(newPath);
        if (oldKey == newKey) return;

        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();

        using var exact = conn.CreateCommand();
        exact.Transaction = tx;
        exact.CommandText =
            "UPDATE OR IGNORE file SET path_key = @newKey, display_path = @newDisplay WHERE path_key = @oldKey;";
        exact.Parameters.AddWithValue("@newKey", newKey);
        exact.Parameters.AddWithValue("@newDisplay", PathKey.NormalizeDisplay(newPath));
        exact.Parameters.AddWithValue("@oldKey", oldKey);
        exact.ExecuteNonQuery();

        // Descendants: rewrite the prefix. Canonical and display forms have equal
        // lengths (canonical is the display form uppercased), so one offset serves both.
        var (lo, hi) = PathKey.PrefixBounds(oldKey);
        using var subtree = conn.CreateCommand();
        subtree.Transaction = tx;
        subtree.CommandText =
            """
            UPDATE OR IGNORE file
            SET path_key = @newKey || substr(path_key, @prefixLen + 1),
                display_path = @newDisplay || substr(display_path, @prefixLen + 1)
            WHERE path_key >= @lo AND path_key < @hi;
            """;
        subtree.Parameters.AddWithValue("@newKey", newKey);
        subtree.Parameters.AddWithValue("@newDisplay", PathKey.NormalizeDisplay(newPath));
        subtree.Parameters.AddWithValue("@prefixLen", oldKey.Length);
        subtree.Parameters.AddWithValue("@lo", lo);
        subtree.Parameters.AddWithValue("@hi", hi);
        subtree.ExecuteNonQuery();

        tx.Commit();
    }

    /// <summary>Removes a file row (and all its tag links) entirely, e.g. "Remove missing".</summary>
    public void RemoveFile(string path)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM file WHERE path_key = @key;";
        cmd.Parameters.AddWithValue("@key", PathKey.Canonicalize(path));
        cmd.ExecuteNonQuery();
    }

    /// <summary>Tags per path, for hydrating chips when a directory listing loads.</summary>
    public IReadOnlyDictionary<string, IReadOnlyList<Tag>> GetTagsForPaths(IReadOnlyCollection<string> paths)
    {
        var result = new Dictionary<string, IReadOnlyList<Tag>>(StringComparer.Ordinal);
        if (paths.Count == 0) return result;

        using var conn = _db.Open();
        foreach (var chunk in paths.Chunk(500))
        {
            using var cmd = conn.CreateCommand();
            cmd.CommandText =
                $"""
                SELECT f.path_key, t.id, t.name, t.color
                FROM file f
                JOIN file_tag ft ON ft.file_id = f.id
                JOIN tag t ON t.id = ft.tag_id
                WHERE f.path_key IN ({ParamList("p", chunk.Length)})
                ORDER BY f.path_key, t.name COLLATE NOCASE;
                """;
            for (var i = 0; i < chunk.Length; i++)
                cmd.Parameters.AddWithValue($"@p{i}", PathKey.Canonicalize(chunk[i]));

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
            {
                var key = reader.GetString(0);
                var tag = new Tag(reader.GetInt64(1), reader.GetString(2), reader.IsDBNull(3) ? null : reader.GetString(3));
                if (result.TryGetValue(key, out var list))
                    ((List<Tag>)list).Add(tag);
                else
                    result[key] = new List<Tag> { tag };
            }
        }
        return result;
    }

    /// <summary>
    /// The flagship recursive query: all tagged files strictly under <paramref name="directory"/>
    /// matching the selected tags (Any = OR, All = AND). DB-driven; never touches the filesystem.
    /// </summary>
    public IReadOnlyList<TaggedFile> QueryTaggedFilesUnder(
        string directory, IReadOnlyCollection<long> tagIds, TagMatchMode mode)
    {
        if (tagIds.Count == 0) return Array.Empty<TaggedFile>();

        var (lo, hi) = PathKey.PrefixBounds(directory);

        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        // Match on the selected tags, but display ALL tags of each matching file.
        cmd.CommandText =
            $"""
            SELECT f.id, f.display_path, group_concat(t.name, char(31)) AS tags
            FROM file f
            JOIN file_tag ft ON ft.file_id = f.id
            JOIN tag t ON t.id = ft.tag_id
            WHERE f.path_key >= @lo AND f.path_key < @hi
              AND f.id IN (
                  SELECT file_id FROM file_tag
                  WHERE tag_id IN ({ParamList("t", tagIds.Count)})
                  GROUP BY file_id
                  {(mode == TagMatchMode.All ? "HAVING COUNT(DISTINCT tag_id) = @tagCount" : "")}
              )
            GROUP BY f.id
            ORDER BY f.display_path;
            """;
        cmd.Parameters.AddWithValue("@lo", lo);
        cmd.Parameters.AddWithValue("@hi", hi);
        var i = 0;
        foreach (var tagId in tagIds)
            cmd.Parameters.AddWithValue($"@t{i++}", tagId);
        if (mode == TagMatchMode.All)
            cmd.Parameters.AddWithValue("@tagCount", tagIds.Count);

        var results = new List<TaggedFile>();
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
        {
            var tags = reader.GetString(2).Split('\u001f');
            Array.Sort(tags, StringComparer.OrdinalIgnoreCase);
            results.Add(new TaggedFile(reader.GetInt64(0), reader.GetString(1), tags));
        }
        return results;
    }

    /// <summary>Per-tag counts of tagged files under a directory, for the filter panel.</summary>
    public IReadOnlyDictionary<long, int> GetTagCountsUnder(string directory)
    {
        var (lo, hi) = PathKey.PrefixBounds(directory);

        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            """
            SELECT ft.tag_id, COUNT(*)
            FROM file_tag ft
            JOIN file f ON f.id = ft.file_id
            WHERE f.path_key >= @lo AND f.path_key < @hi
            GROUP BY ft.tag_id;
            """;
        cmd.Parameters.AddWithValue("@lo", lo);
        cmd.Parameters.AddWithValue("@hi", hi);

        var counts = new Dictionary<long, int>();
        using var reader = cmd.ExecuteReader();
        while (reader.Read())
            counts[reader.GetInt64(0)] = reader.GetInt32(1);
        return counts;
    }

    private static void DeleteOrphanFiles(SqliteCommand cmd)
    {
        cmd.Parameters.Clear();
        cmd.CommandText = "DELETE FROM file WHERE id NOT IN (SELECT DISTINCT file_id FROM file_tag);";
        cmd.ExecuteNonQuery();
    }

    private static string ParamList(string prefix, int count) =>
        string.Join(", ", Enumerable.Range(0, count).Select(i => $"@{prefix}{i}"));
}
