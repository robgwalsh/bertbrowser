using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class BenchResultsJsonTests
{
    [Fact]
    public void RoundTrips()
    {
        var entry = new BenchEntry(
            "ui.listing-10k.list", "B",
            new Dictionary<string, string> { ["files"] = "10000", ["dirs"] = "50" },
            5,
            new TimeStats("ms", 812.4, 790, 905, 48.2, 770, 905),
            null,
            new ProcessStats(412_000_000, 380_000_000, 96_000_000, 38, 10_000, 0),
            null);
        var set = BenchCompareTests.Set([entry, BenchCompareTests.Entry("core.x.y", 123.4, 2048)], real: true);

        var json = BenchResultsJson.Serialize(set);
        var back = BenchResultsJson.Deserialize(json);

        Assert.Equal(set.SchemaVersion, back.SchemaVersion);
        Assert.Equal(set.GeneratedUtc, back.GeneratedUtc);
        Assert.Equal(set.Tool, back.Tool);
        Assert.Equal(set.Git, back.Git);
        Assert.Equal(set.Machine, back.Machine);
        Assert.Equal(set.Run with { Tiers = [] }, back.Run with { Tiers = [] });
        Assert.Equal(set.Run.Tiers, back.Run.Tiers);
        Assert.Equal(2, back.Benchmarks.Count);
        Assert.Equal(entry.Parameters["files"], back.Benchmarks[0].Parameters["files"]);
        Assert.Equal(entry.Time, back.Benchmarks[0].Time);
        Assert.Equal(entry.Process, back.Benchmarks[0].Process);
        Assert.Null(back.Benchmarks[0].Memory);
        Assert.Equal(set.Benchmarks[1].Memory, back.Benchmarks[1].Memory);
        Assert.True(back.Run.Real);
    }

    [Fact]
    public void Serialize_IsCamelCaseAndOmitsNulls()
    {
        var json = BenchResultsJson.Serialize(BenchCompareTests.Set([BenchCompareTests.Entry("a", 1, null)], false));
        Assert.Contains("\"schemaVersion\": 1", json);
        Assert.Contains("\"benchmarks\"", json);
        Assert.DoesNotContain("\"memory\"", json);
        Assert.DoesNotContain("\"error\"", json);
        Assert.DoesNotContain("SchemaVersion", json);
    }

    [Fact]
    public void Deserialize_WrongSchemaVersion_Throws()
    {
        var json = BenchResultsJson.Serialize(BenchCompareTests.Set([], false))
            .Replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        var e = Assert.Throws<InvalidDataException>(() => BenchResultsJson.Deserialize(json));
        Assert.Contains("schema version 2", e.Message);
    }

    [Fact]
    public void Deserialize_Garbage_Throws()
    {
        Assert.Throws<InvalidDataException>(() => BenchResultsJson.Deserialize("not json"));
        Assert.Throws<InvalidDataException>(() => BenchResultsJson.Deserialize("null"));
    }

    [Fact]
    public void Write_CreatesDirectory()
    {
        var dir = Path.Combine(Path.GetTempPath(), $"bertbrowser-bench-test-{Guid.NewGuid():N}");
        try
        {
            var path = Path.Combine(dir, "nested", "results.json");
            BenchResultsJson.Write(BenchCompareTests.Set([], false), path);
            Assert.True(File.Exists(path));
            Assert.Empty(BenchResultsJson.Read(path).Benchmarks);
        }
        finally
        {
            if (Directory.Exists(dir)) Directory.Delete(dir, recursive: true);
        }
    }
}
