using System.Globalization;
using Microsoft.Data.Sqlite;

namespace BertBrowser.Core.Data;

/// <summary>The round-trip ("O") UTC text the saved-search and saved-workspace tables keep their
/// timestamps in, both ways.</summary>
internal static class StoredTime
{
    public static string Write(DateTime utc) => utc.ToUniversalTime().ToString("O", CultureInfo.InvariantCulture);

    /// <summary>The stored time as UTC, or null when absent or unreadable — a bad date costs a row
    /// its metadata, never its place in the list.</summary>
    public static DateTime? Read(SqliteDataReader reader, int ordinal)
    {
        if (reader.IsDBNull(ordinal)) return null;
        return DateTime.TryParse(reader.GetString(ordinal), CultureInfo.InvariantCulture,
            DateTimeStyles.RoundtripKind, out var parsed)
            ? parsed.ToUniversalTime()
            : null;
    }
}
