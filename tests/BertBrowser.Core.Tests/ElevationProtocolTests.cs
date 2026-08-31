using BertBrowser.Core.Cli;
using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The line format the elevated file-operation helper speaks. Everything crossing it is untrusted,
/// so the only acceptable behaviour for a malformed line is a refusal — never a throw, and never a
/// partial read.
/// </summary>
public class ElevationProtocolTests
{
    [Theory]
    [InlineData(ElevationVerb.Ready, "")]
    [InlineData(ElevationVerb.Go, "")]
    [InlineData(ElevationVerb.Cancel, "")]
    [InlineData(ElevationVerb.Hello, "1")]
    [InlineData(ElevationVerb.Begin, "{\"Operation\":0}")]
    [InlineData(ElevationVerb.Fatal, "it went wrong")]
    public void RoundTrips(ElevationVerb verb, string payload)
    {
        var line = ElevationProtocol.Format(new ElevationMessage(verb, payload));

        Assert.True(ElevationProtocol.TryParse(line, out var parsed));
        Assert.Equal(verb, parsed.Verb);
        Assert.Equal(payload, parsed.Payload);
    }

    [Fact]
    public void APayloadContainingATabSurvives()
    {
        // The format splits into at most three fields, so a payload may hold a separator without
        // being cut in half — which matters because JSON escapes a tab inside a string rather than
        // removing it.
        var payload = "{\"Name\":\"a\\tb\"}";

        Assert.True(ElevationProtocol.TryParse(
            ElevationProtocol.Format(new ElevationMessage(ElevationVerb.Item, payload)), out var parsed));
        Assert.Equal(payload, parsed.Payload);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("OPS")]
    [InlineData("IDX\tReady")]
    [InlineData("ops\tReady")]
    [InlineData("OPS\tSomethingElse")]
    [InlineData("OPS\t3")]
    [InlineData("OPS\tDone, Fault")]
    public void RefusesALineThatIsNotOneOfOurs(string? line) =>
        Assert.False(ElevationProtocol.TryParse(line, out _));

    [Theory]
    [InlineData(ElevationVerb.Ready)]
    [InlineData(ElevationVerb.Go)]
    [InlineData(ElevationVerb.Cancel)]
    public void TheVerbsThatCarryNothingCarryNothing(ElevationVerb verb) =>
        Assert.False(ElevationProtocol.IsAcceptablePayload(verb, "{}"));

    [Theory]
    [InlineData(ElevationVerb.Begin)]
    [InlineData(ElevationVerb.Item)]
    [InlineData(ElevationVerb.Done)]
    [InlineData(ElevationVerb.Fault)]
    [InlineData(ElevationVerb.End)]
    [InlineData(ElevationVerb.Progress)]
    public void TheVerbsThatCarryARecordNeedOne(ElevationVerb verb) =>
        Assert.False(ElevationProtocol.IsAcceptablePayload(verb, ""));

    [Theory]
    [InlineData("0")]
    [InlineData("-1")]
    [InlineData("one")]
    [InlineData("")]
    public void AGreetingWithoutAVersionIsRefused(string payload) =>
        Assert.False(ElevationProtocol.IsAcceptablePayload(ElevationVerb.Hello, payload));

    [Fact]
    public void ALineLongerThanTheCapIsRefused() =>
        Assert.False(ElevationProtocol.TryParse(
            "OPS\tItem\t" + new string('x', NavigationRequest.MaxLineLength), out _));

    [Fact]
    public void TheItemCapIsWhatBoundsARequestRatherThanTheLineLength()
    {
        // The two bounds are in different dimensions on purpose: one record per line keeps the line
        // cap where LineChannel put it, and the count keeps the request finite. A cap that has to be
        // raised to fit a big operation is not a cap.
        Assert.True(ElevationProtocol.MaxItems > 0);
        Assert.Equal(64 * 1024, NavigationRequest.MaxLineLength);
    }

    [Fact]
    public void AStatusIsFlattenedOntoOneLineAndBounded()
    {
        var summary = ElevationProtocol.Summarize("something\r\nbroke\tbadly" + new string('!', 500));

        Assert.DoesNotContain('\n', summary);
        Assert.DoesNotContain('\t', summary);
        Assert.True(summary.Length <= ElevationProtocol.MaxStatusLength);
        Assert.True(ElevationProtocol.IsAcceptablePayload(ElevationVerb.Fatal, summary));
    }

    [Fact]
    public void AnEmptyStatusStillSaysSomething() =>
        Assert.NotEmpty(ElevationProtocol.Summarize("   "));

    // --- the helper's command line ---

    [Fact]
    public void TheHelperTakesAPipeAParentAndASid()
    {
        Assert.True(ElevatorArguments.TryParse(
            ["--pipe", "BertBrowser.Elevate.ABC", "--parent-pid", "42", "--user-sid", "S-1-5-21-1-2-3"],
            out var parsed, out _));

        Assert.Equal("BertBrowser.Elevate.ABC", parsed.PipeName);
        Assert.Equal(42, parsed.ParentProcessId);
        Assert.Equal("S-1-5-21-1-2-3", parsed.UserSid);
    }

    [Fact]
    public void TheHelperRefusesAnUnrecognisedArgument() =>
        Assert.False(ElevatorArguments.TryParse(
            ["--pipe", "BertBrowser.Elevate.ABC", "--parent-pid", "42", "--user-sid", "S-1-5-1", "--data-dir", @"C:\x"],
            out _, out _));

    [Fact]
    public void TheHelperRefusesAnIndexPipe() =>
        // Separate prefixes so neither helper can ever be handed the other's endpoint.
        Assert.False(ElevatorArguments.IsAcceptablePipeName("BertBrowser.Index.ABC"));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData(@"BertBrowser.Elevate.\..\evil")]
    [InlineData("BertBrowser.Elevate.a:b")]
    [InlineData("Something.Else")]
    public void TheHelperRefusesAnEndpointItShouldNotTouch(string? name) =>
        Assert.False(ElevatorArguments.IsAcceptablePipeName(name));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("Rob")]
    [InlineData("S-1-5-21-../x")]
    public void TheHelperRefusesSomethingThatIsNotASid(string? sid) =>
        Assert.False(ElevatorArguments.IsAcceptableSid(sid));
}
