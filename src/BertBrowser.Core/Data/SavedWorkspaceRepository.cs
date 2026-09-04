using System.Text.Json;
using BertBrowser.Core.Layout;
using BertBrowser.Core.Models;

namespace BertBrowser.Core.Data;

/// <summary>Synchronous ADO.NET store for saved workspaces, keyed by name (ignoring case).</summary>
public sealed class SavedWorkspaceRepository
{
    private readonly Db _db;

    public SavedWorkspaceRepository(Db db) => _db = db;

    /// <summary>Every saved workspace, ordered by name ignoring case — the sidebar's order. A row
    /// whose layout_json fails to deserialize (hand-edited DB, a future format change) is skipped
    /// rather than failing the whole list.</summary>
    public IReadOnlyList<SavedWorkspace> GetAll()
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            "SELECT name, layout_json FROM saved_workspace ORDER BY name COLLATE NOCASE;";
        using var reader = cmd.ExecuteReader();
        var list = new List<SavedWorkspace>();
        while (reader.Read())
        {
            var name = reader.GetString(0);
            SessionLayout? layout;
            try
            {
                layout = JsonSerializer.Deserialize<SessionLayout>(reader.GetString(1));
            }
            catch (JsonException)
            {
                continue;
            }
            if (layout is null) continue;
            list.Add(new SavedWorkspace(name, layout));
        }
        return list;
    }

    /// <summary>Creates the workspace, or replaces the layout of the one already stored under that
    /// name (in any case). The original added_utc is kept on a replace; the stored name takes the
    /// casing of the row that was there first, since NOCASE treats them as one key.</summary>
    public void Save(SavedWorkspace workspace)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText =
            """
            INSERT INTO saved_workspace(name, layout_json, added_utc)
            VALUES (@name, @layout, @now)
            ON CONFLICT(name) DO UPDATE SET
                layout_json = excluded.layout_json;
            """;
        cmd.Parameters.AddWithValue("@name", workspace.Name);
        cmd.Parameters.AddWithValue("@layout", JsonSerializer.Serialize(workspace.Layout));
        cmd.Parameters.AddWithValue("@now", DateTime.UtcNow.ToString("O"));
        cmd.ExecuteNonQuery();
    }

    /// <summary>Renames a workspace. Returns false, changing nothing, when there is no such row or
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
            check.CommandText = "SELECT 1 FROM saved_workspace WHERE name = @name LIMIT 1;";
            check.Parameters.AddWithValue("@name", newName);
            if (check.ExecuteScalar() is not null) return false;
        }

        using var cmd = conn.CreateCommand();
        cmd.CommandText = "UPDATE saved_workspace SET name = @new WHERE name = @old;";
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
        cmd.CommandText = "DELETE FROM saved_workspace WHERE name = @name;";
        cmd.Parameters.AddWithValue("@name", name);
        cmd.ExecuteNonQuery();
    }

    public bool Exists(string name)
    {
        using var conn = _db.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = "SELECT 1 FROM saved_workspace WHERE name = @name LIMIT 1;";
        cmd.Parameters.AddWithValue("@name", name);
        return cmd.ExecuteScalar() is not null;
    }
}
