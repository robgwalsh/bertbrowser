using System.Text.Json;
using System.Text.Json.Serialization;

namespace BertBrowser.Core.Benchmarking;

/// <summary>Reads and writes <see cref="BenchResultSet"/> files.</summary>
/// <remarks>
/// camelCase, indented, nulls omitted: the files are committed as baselines and read in diffs, so
/// they are formatted for a person first. A file from a different schema version is refused rather
/// than half-read — a silently missing field would compare as "no regression".
/// </remarks>
public static class BenchResultsJson
{
    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static string Serialize(BenchResultSet set) => JsonSerializer.Serialize(set, Options);

    public static BenchResultSet Deserialize(string json)
    {
        BenchResultSet? set;
        try
        {
            set = JsonSerializer.Deserialize<BenchResultSet>(json, Options);
        }
        catch (JsonException e)
        {
            throw new InvalidDataException($"Not a benchmark results file: {e.Message}", e);
        }

        if (set is null) throw new InvalidDataException("Not a benchmark results file: empty document.");
        if (set.SchemaVersion != BenchResultSet.CurrentSchemaVersion)
        {
            throw new InvalidDataException(
                $"Results file is schema version {set.SchemaVersion}; this build reads version " +
                $"{BenchResultSet.CurrentSchemaVersion}.");
        }

        return set;
    }

    public static void Write(BenchResultSet set, string path)
    {
        var dir = Path.GetDirectoryName(Path.GetFullPath(path));
        if (dir is not null) Directory.CreateDirectory(dir);
        File.WriteAllText(path, Serialize(set) + Environment.NewLine);
    }

    public static BenchResultSet Read(string path) => Deserialize(File.ReadAllText(path));
}
