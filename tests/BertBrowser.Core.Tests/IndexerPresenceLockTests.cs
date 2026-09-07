using BertBrowser.Core.Ipc;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The name and the descriptor are interpolated from a SID, and both are parsed by the kernel — so
/// what a SID is allowed to be is the whole of the safety here.
/// </summary>
public class IndexerPresenceLockTests
{
    private const string Sid = "S-1-5-21-1845729561-1936007304-1872547691-1001";

    [Fact]
    public void NamesTheMutexPerSessionAndPerUser()
    {
        // Local\, not Global\: two users signed in at once must not share one helper's name.
        Assert.Equal($@"Local\BertBrowser.Indexer.{Sid}", IndexerPresenceLock.NameFor(Sid));
    }

    [Fact]
    public void GrantsTheUserSynchroniseAndNothingMore()
    {
        var sddl = IndexerPresenceLock.Sddl(Sid);

        // 0x00100000 is SYNCHRONIZE — enough to answer "is it running?", not enough to acquire it.
        Assert.Equal($"D:(A;;0x00100000;;;{Sid})(A;;GA;;;BA)(A;;GA;;;SY)", sddl);
    }

    [Theory]
    [InlineData("S-1-5-21-1-2-3-1001")]
    [InlineData("S-1-1-0")]
    public void AcceptsARealSid(string candidate)
    {
        Assert.True(IndexerPresenceLock.IsAcceptableSid(candidate));
    }

    /// <summary>
    /// The SID lands inside an SDDL string and a kernel object name. A value carrying a bracket
    /// would rewrite the descriptor; one carrying a separator would name a different object.
    /// </summary>
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("S-")]
    [InlineData(@"S-1-5-21-1001\..\Global\Evil")]
    [InlineData("S-1-5-21-1001)(A;;GA;;;WD")]
    [InlineData("S-1-5-21-1001;GA")]
    [InlineData("NotASid")]
    [InlineData("S-1-5-21-1001 ")]
    public void RefusesAnythingElse(string? candidate)
    {
        Assert.False(IndexerPresenceLock.IsAcceptableSid(candidate));
    }

    [Fact]
    public void RefusesToBuildANameFromABadSid()
    {
        Assert.Throws<ArgumentException>(() => IndexEndpoint.ForUser("S-1-5-21-1001)(A;;GA;;;WD"));
    }

    [Fact]
    public void TheEndpointIsThePrefixAndTheSid()
    {
        Assert.Equal($"BertBrowser.Index.{Sid}", IndexEndpoint.ForUser(Sid));
    }

    /// <summary>
    /// The real thing, end to end: claiming the name makes it visible, a second claim is refused,
    /// and letting go makes it invisible again. This is what tells the app whether to raise a
    /// prompt, and what stops two helpers indexing at once.
    /// </summary>
    [Fact]
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    public void AClaimIsVisibleUntilItIsReleased()
    {
        var sid = IndexEndpoint.CurrentUserSid();
        Assert.False(IndexerPresenceLock.IsHeld(sid), "nothing should hold it before the test starts");

        using (var claim = IndexerPresenceLock.TryAcquire(sid))
        {
            Assert.NotNull(claim);
            Assert.True(IndexerPresenceLock.IsHeld(sid));
            Assert.Null(IndexerPresenceLock.TryAcquire(sid));
        }

        Assert.False(IndexerPresenceLock.IsHeld(sid));
    }
}
