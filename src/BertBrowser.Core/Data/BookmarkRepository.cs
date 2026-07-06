using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Data;

/// <summary>Synchronous ADO.NET store for sidebar bookmarks, keyed by canonical path.</summary>
public sealed class BookmarkRepository
{
    private readonly Db _db;

    public BookmarkRepository(Db db) => _db = db;

    /// <summary>All bookmarks, ordered for display (folders before files, then by name).</summary>
    public IReadOnlyList<Bookmark> GetAll()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            "SELECT display_path, is_directory FROM bookmark ORDER BY is_directory DESC, display_path COLLATE NOCASE;";
        using var reader = cmd.ExecuteReader();
        var bookmarks = new List<Bookmark>();
        while (reader.Read())
            bookmarks.Add(new Bookmark(reader.GetString(0), reader.GetInt64(1) != 0));
        return bookmarks;
    }

    /// <summary>Adds a bookmark. Returns false if the path was already bookmarked.</summary>
    public bool Add(string path, bool isDirectory)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            """
            INSERT OR IGNORE INTO bookmark(path_key, display_path, is_directory, added_utc)
            VALUES (@key, @display, @dir, @now);
            """;
        cmd.Parameters.AddWithValue("@key", PathKey.Canonicalize(path));
        cmd.Parameters.AddWithValue("@display", PathKey.NormalizeDisplay(path));
        cmd.Parameters.AddWithValue("@dir", isDirectory ? 1 : 0);
        cmd.Parameters.AddWithValue("@now", DateTime.UtcNow.ToString("O"));
        return cmd.ExecuteNonQuery() > 0;
    }

    public void Remove(string path)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM bookmark WHERE path_key = @key;";
        cmd.Parameters.AddWithValue("@key", PathKey.Canonicalize(path));
        cmd.ExecuteNonQuery();
    }

    public bool Exists(string path)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT 1 FROM bookmark WHERE path_key = @key LIMIT 1;";
        cmd.Parameters.AddWithValue("@key", PathKey.Canonicalize(path));
        return cmd.ExecuteScalar() is not null;
    }
}
