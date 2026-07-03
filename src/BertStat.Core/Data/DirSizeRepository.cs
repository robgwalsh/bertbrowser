using BertStat.Core.Models;
using BertStat.Core.Paths;

namespace BertStat.Core.Data;

public sealed class DirSizeRepository
{
    private readonly Db _db;

    public DirSizeRepository(Db db) => _db = db;

    /// <summary>Upserts an entire scanned subtree in one transaction.</summary>
    public void UpsertMany(IEnumerable<DirSizeResult> results)
    {
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();
        using var cmd = conn.CreateCommand();
        cmd.Transaction = tx;
        cmd.CommandText =
            """
            INSERT INTO dir_size_cache(path_key, size_bytes, file_count, dir_count, incomplete, computed_utc)
            VALUES (@key, @size, @files, @dirs, @incomplete, @computed)
            ON CONFLICT(path_key) DO UPDATE SET
                size_bytes = excluded.size_bytes,
                file_count = excluded.file_count,
                dir_count = excluded.dir_count,
                incomplete = excluded.incomplete,
                computed_utc = excluded.computed_utc;
            """;
        var pKey = cmd.Parameters.Add("@key", Microsoft.Data.Sqlite.SqliteType.Text);
        var pSize = cmd.Parameters.Add("@size", Microsoft.Data.Sqlite.SqliteType.Integer);
        var pFiles = cmd.Parameters.Add("@files", Microsoft.Data.Sqlite.SqliteType.Integer);
        var pDirs = cmd.Parameters.Add("@dirs", Microsoft.Data.Sqlite.SqliteType.Integer);
        var pInc = cmd.Parameters.Add("@incomplete", Microsoft.Data.Sqlite.SqliteType.Integer);
        var pComputed = cmd.Parameters.Add("@computed", Microsoft.Data.Sqlite.SqliteType.Text);

        foreach (var r in results)
        {
            pKey.Value = r.PathKey;
            pSize.Value = r.SizeBytes;
            pFiles.Value = r.FileCount;
            pDirs.Value = r.DirCount;
            pInc.Value = r.Incomplete ? 1 : 0;
            pComputed.Value = r.ComputedUtc.ToString("O");
            cmd.ExecuteNonQuery();
        }

        tx.Commit();
    }

    /// <summary>Cached sizes for a set of directories (typically the children of the current dir).</summary>
    public IReadOnlyDictionary<string, DirSizeResult> GetMany(IReadOnlyCollection<string> paths)
    {
        var result = new Dictionary<string, DirSizeResult>(StringComparer.Ordinal);
        if (paths.Count == 0) return result;

        using var conn = _db.Open();
        foreach (var chunk in paths.Chunk(500))
        {
            using var cmd = conn.CreateCommand();
            var parms = string.Join(", ", Enumerable.Range(0, chunk.Length).Select(i => $"@p{i}"));
            cmd.CommandText =
                $"""
                SELECT path_key, size_bytes, file_count, dir_count, incomplete, computed_utc
                FROM dir_size_cache
                WHERE path_key IN ({parms});
                """;
            for (var i = 0; i < chunk.Length; i++)
                cmd.Parameters.AddWithValue($"@p{i}", PathKey.Canonicalize(chunk[i]));

            using var reader = cmd.ExecuteReader();
            while (reader.Read())
            {
                var row = new DirSizeResult(
                    reader.GetString(0),
                    reader.GetInt64(1),
                    reader.GetInt32(2),
                    reader.GetInt32(3),
                    reader.GetInt32(4) != 0,
                    DateTime.Parse(reader.GetString(5), null, System.Globalization.DateTimeStyles.RoundtripKind));
                result[row.PathKey] = row;
            }
        }
        return result;
    }

    public DirSizeResult? Get(string path)
    {
        var key = PathKey.Canonicalize(path);
        return GetMany(new[] { key }).TryGetValue(key, out var r) ? r : null;
    }
}
