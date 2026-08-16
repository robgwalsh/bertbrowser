using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The single-instance endpoint name. What is being defended here is not the app against its own
/// user — it cannot be — but one signed-in account against another: pipe names are machine-wide and
/// first-come, so a guessable name is one somebody else can take. Mutate
/// <see cref="InstanceEndpoint.IsAcceptable"/> to wave a name through and the theories below go red.
/// </summary>
public class InstanceEndpointTests
{
    private const string Sid = "S-1-5-21-1111111111-2222222222-3333333333-1001";
    private const string Nonce = "0123456789ABCDEF0123456789ABCDEF";

    [Fact]
    public void AGeneratedNameIsAccepted()
    {
        var name = InstanceEndpoint.Name(Sid, Nonce);

        Assert.Equal($"BertBrowser.{Sid}.{Nonce}", name);
        Assert.True(InstanceEndpoint.IsAcceptable(name, Sid));
    }

    /// <summary>
    /// The whole point of the design: without the nonce the name is derivable from the SID alone,
    /// which is public. Accepting a bare prefix would quietly restore the guessable endpoint.
    /// </summary>
    [Fact]
    public void ANameCarryingNoNonceIsRefused()
    {
        Assert.False(InstanceEndpoint.IsAcceptable($"BertBrowser.{Sid}", Sid));
        Assert.False(InstanceEndpoint.IsAcceptable($"BertBrowser.{Sid}.", Sid));
    }

    [Theory]
    [InlineData("0123456789ABCDEF0123456789ABCDE")]    // one short
    [InlineData("0123456789ABCDEF0123456789ABCDEF0")]  // one long
    [InlineData("0123456789abcdef0123456789abcdef")]   // lower case is not what we emit
    [InlineData("0123456789ABCDEF0123456789ABCDEG")]   // 'G' is not hex
    public void ANonceOfTheWrongShapeIsRefused(string nonce) =>
        Assert.False(InstanceEndpoint.IsAcceptable($"BertBrowser.{Sid}.{nonce}", Sid));

    /// <summary>
    /// The name is interpolated into <c>\\.\pipe\</c>, so anything that could steer it somewhere
    /// else has to fail. These all do already — the nonce rule admits hex and nothing else — and the
    /// theory is here so that stays true if the shape is ever loosened.
    /// </summary>
    [Theory]
    [InlineData(@"..\..\evil")]
    [InlineData(@"sub\path")]
    [InlineData("wild*card")]
    [InlineData("with space")]
    [InlineData("new\nline")]
    [InlineData("nul\0byte")]
    public void ANameThatCouldNameSomethingElseIsRefused(string nonce) =>
        Assert.False(InstanceEndpoint.IsAcceptable($"BertBrowser.{Sid}.{nonce}", Sid));

    [Fact]
    public void AnotherAccountsEndpointIsRefused()
    {
        var theirs = InstanceEndpoint.Name("S-1-5-21-9-9-9-500", Nonce);

        Assert.False(InstanceEndpoint.IsAcceptable(theirs, Sid));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("BertBrowser.Index.ABC")]  // the other pipe in this app
    [InlineData("Something.Else")]
    public void AnEndpointThisAppNeverProducedIsRefused(string? candidate) =>
        Assert.False(InstanceEndpoint.IsAcceptable(candidate, Sid));

    [Fact]
    public void AnUnusableUserKeyIsRefusedRatherThanMatchingEverything()
    {
        var name = InstanceEndpoint.Name(Sid, Nonce);

        Assert.False(InstanceEndpoint.IsAcceptable(name, null));
        Assert.False(InstanceEndpoint.IsAcceptable(name, ""));
    }

    [Fact]
    public void AnOversizedNameIsRefused()
    {
        var huge = new string('A', InstanceEndpoint.MaxNameLength);

        Assert.False(InstanceEndpoint.IsAcceptable($"BertBrowser.{Sid}.{huge}", Sid));
    }
}
