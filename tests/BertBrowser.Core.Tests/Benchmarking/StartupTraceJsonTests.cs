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
