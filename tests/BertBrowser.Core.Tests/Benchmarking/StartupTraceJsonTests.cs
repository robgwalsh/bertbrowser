using BertBrowser.Core.Benchmarking;
using Xunit;

namespace BertBrowser.Core.Tests.Benchmarking;

public class StartupTraceJsonTests
{
    [Fact]
    public void RoundTrips()
    {
        var trace = new StartupTraceData(
            1234,
            new DateTime(2026, 10, 2, 12, 0, 0, DateTimeKind.Utc),
            new Dictionary<string, double> { [StartupMarks.Main] = 41.5, [StartupMarks.FirstListing] = 1210.25 },
            "first",
            "skipped",
            SideEffectsSkipped: true,
            Error: null);

        var back = StartupTraceJson.Deserialize(StartupTraceJson.Serialize(trace));

        Assert.Equal(trace.Pid, back.Pid);
        Assert.Equal(trace.ProcessStartUtc, back.ProcessStartUtc);
        Assert.Equal(41.5, back.MarksMs[StartupMarks.Main]);
        Assert.Equal(1210.25, back.MarksMs[StartupMarks.FirstListing]);
        Assert.Equal("first", back.Instance);
        Assert.True(back.SideEffectsSkipped);
        Assert.Null(back.Error);
    }

    [Fact]
    public void Memory_RoundTrips_AndIsAbsentFromAnOlderTrace()
    {
        var reading = new StartupMemory(96_000_000, 61_000_000, 9_500_000, 104_000_000);
        var trace = new StartupTraceData(1, DateTime.UnixEpoch, new Dictionary<string, double>(), "first", "skipped", true, null,
            new Dictionary<string, StartupMemory> { [StartupMemoryPoints.Collected] = reading });

        var json = StartupTraceJson.Serialize(trace);
        Assert.Contains("\"managedHeapBytes\": 9500000", json);
        Assert.Equal(reading, StartupTraceJson.Deserialize(json).Memory![StartupMemoryPoints.Collected]);

        // A trace from a build that did not read its memory has no such property at all.
        var older = StartupTraceJson.Serialize(trace with { Memory = null });
        Assert.DoesNotContain("\"memory\"", older);
        Assert.Null(StartupTraceJson.Deserialize(older).Memory);
    }

    [Fact]
    public void Serialize_IsCamelCase()
    {
        var json = StartupTraceJson.Serialize(new StartupTraceData(1, DateTime.UnixEpoch, new Dictionary<string, double>(), "second", "skipped", true, "late"));
        Assert.Contains("\"marksMs\"", json);
        Assert.Contains("\"sideEffectsSkipped\": true", json);
        Assert.Contains("\"error\": \"late\"", json);
    }

    [Fact]
    public void Deserialize_Empty_Throws() =>
        Assert.Throws<InvalidDataException>(() => StartupTraceJson.Deserialize("null"));
}
