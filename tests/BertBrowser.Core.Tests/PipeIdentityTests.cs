using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The comparison that fails <em>closed</em> when it is wrong: a pipe that accepts every connection
/// and drops it, rather than an error anyone would notice.
/// </summary>
public class PipeIdentityTests
{
    /// <summary>The exact pair the two Windows APIs actually hand back.</summary>
    [Theory]
    [InlineData("Rob", "DESKTOP-K0BI3BS\\Rob")]
    [InlineData("DESKTOP-K0BI3BS\\Rob", "Rob")]
    [InlineData("Rob", "Rob")]
    [InlineData("CONTOSO\\Rob", "DESKTOP-K0BI3BS\\Rob")]
    public void TheTwoFormsOfTheSameAccountMatch(string left, string right)
    {
        Assert.True(PipeIdentity.SameAccount(left, right));
    }

    [Theory]
    [InlineData("Rob", "Sam")]
    [InlineData("DESKTOP\\Rob", "DESKTOP\\Sam")]
    [InlineData("Rob", "DESKTOP\\Robert")]
    [InlineData("Rob", "")]
    public void DifferentAccountsDoNotMatch(string left, string right)
    {
        Assert.False(PipeIdentity.SameAccount(left, right));
    }

    /// <summary>Windows account names are case-insensitive.</summary>
    [Fact]
    public void ComparisonIgnoresCase()
    {
        Assert.True(PipeIdentity.SameAccount("rob", "DESKTOP\\ROB"));
    }

    [Fact]
    public void AnUnknownIdentityDoesNotMatch()
    {
        Assert.False(PipeIdentity.SameAccount(null, "DESKTOP\\Rob"));
        Assert.False(PipeIdentity.SameAccount("Rob", null));
        Assert.False(PipeIdentity.SameAccount(null, null));
    }

    [Theory]
    [InlineData("Rob", "Rob")]
    [InlineData("DESKTOP\\Rob", "Rob")]
    [InlineData("CONTOSO\\sub\\Rob", "Rob")]
    [InlineData("", "")]
    public void AccountPartDropsAnyDomainPrefix(string name, string expected)
    {
        Assert.Equal(expected, PipeIdentity.AccountPart(name));
    }
}
