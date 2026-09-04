using BertBrowser.Core.Models;
using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Data;

/// <summary>Synchronous ADO.NET store for saved searches, keyed by name (ignoring case).</summary>
public sealed class SavedSearchRepository
{
    private readonly Db _db;

    public SavedSearchRepository(Db db) => _db = db;

    /// <summary>Every saved search, ordered by name ignoring case — the sidebar's order.</summary>
    public IReadOnlyList<SavedSearch> GetAll()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            "SELECT name, query, scope, scope_path FROM saved_search ORDER BY name COLLATE NOCASE;";
        using var reader = cmd.ExecuteReader();
        var list = new List<SavedSearch>();
        while (reader.Read())
        {
            list.Add(new SavedSearch(
                reader.GetString(0),
                reader.GetString(1),
                (SavedSearchScope)reader.GetInt64(2),
                reader.IsDBNull(3) ? null : reader.GetString(3)));
        }
        return list;
    }

    /// <summary>Creates the search, or replaces the query and scope of the one already stored under
    /// that name (in any case). The original added_utc is kept on a replace; the stored name takes
    /// the casing of the row that was there first, since NOCASE treats them as one key.</summary>
    public void Save(SavedSearch search)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            """
            INSERT INTO saved_search(name, query, scope, scope_path, added_utc)
            VALUES (@name, @query, @scope, @path, @now)
            ON CONFLICT(name) DO UPDATE SET
                query = excluded.query,
                scope = excluded.scope,
                scope_path = excluded.scope_path;
            """;
        cmd.Parameters.AddWithValue("@name", search.Name);
        cmd.Parameters.AddWithValue("@query", search.Query);
        cmd.Parameters.AddWithValue("@scope", (long)search.Scope);
        cmd.Parameters.AddWithValue("@path",
            search.ScopePath is null ? DBNull.Value : PathKey.NormalizeDisplay(search.ScopePath));
        cmd.Parameters.AddWithValue("@now", DateTime.UtcNow.ToString("O"));
        cmd.ExecuteNonQuery();
    }

    /// <summary>Renames a search. Returns false, changing nothing, when there is no such row or
    /// when <paramref name="newName"/> already belongs to a different row. A change of case only
    /// is a rename of the same row and is allowed.</summary>
    public bool Rename(string oldName, string newName)
    {
        using var conn = _db.Open();
        using var tx = conn.BeginTransaction();

        var sameRow = string.Equals(oldName, newName, StringComparison.OrdinalIgnoreCase);
        if (!sameRow)
        {
            using var check = conn.CreateCommand();
            check.CommandText = "SELECT 1 FROM saved_search WHERE name = @name LIMIT 1;";
            check.Parameters.AddWithValue("@name", newName);
            if (check.ExecuteScalar() is not null) return false;
        }

        using var cmd = conn.CreateCommand();
        cmd.CommandText = "UPDATE saved_search SET name = @new WHERE name = @old;";
        cmd.Parameters.AddWithValue("@new", newName);
        cmd.Parameters.AddWithValue("@old", oldName);
        var moved = cmd.ExecuteNonQuery() > 0;
        tx.Commit();
        return moved;
    }

    public void Remove(string name)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "DELETE FROM saved_search WHERE name = @name;";
        cmd.Parameters.AddWithValue("@name", name);
        cmd.ExecuteNonQuery();
    }

    public bool Exists(string name)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT 1 FROM saved_search WHERE name = @name LIMIT 1;";
        cmd.Parameters.AddWithValue("@name", name);
        return cmd.ExecuteScalar() is not null;
    }
}
