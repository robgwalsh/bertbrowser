using BertBrowser.Core.Services.Mft;
using Xunit;

namespace BertBrowser.Core.Tests;

/// <summary>
/// The banner replaced an elevation prompt that used to appear on every launch, so most of what
/// matters here is when it stays quiet. A strip nobody can act on, or one repeating what the status
/// bar already says, is worse than the prompt it replaced.
/// </summary>
public class IndexerBannerRulesTests
{
    private static IndexerBanner Decide(
        IndexerPresence presence = IndexerPresence.NotRunning,
        bool canStart = true,
        bool dismissed = false,
        string failure = "") =>
        IndexerBannerRules.Decide(presence, canStart, dismissed, failure);

    [Fact]
    public void OffersToStartAHelperWhenThereIsNoneAndOneCouldBeStarted()
    {
        var banner = Decide();

        Assert.True(banner.Show);
        Assert.True(banner.CanStart);
        Assert.Equal(IndexerBannerRules.Offer, banner.Message);
    }

    /// <summary>It has to say what is missing, what it costs, and that it is not every launch.</summary>
    [Fact]
    public void TheOfferExplainsWhyAdministratorRightsAreWanted()
    {
        Assert.Contains("administrator rights", IndexerBannerRules.Offer, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("once", IndexerBannerRules.Offer, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("search", IndexerBannerRules.Offer, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void SaysNothingWhileAHelperIsRunning()
    {
        Assert.False(Decide(presence: IndexerPresence.Running).Show);
    }

    /// <summary>
    /// The in-process indexer and the UI harness have no helper to miss. Reporting one absent would
    /// put this strip across every scripted screenshot.
    /// </summary>
    [Fact]
    public void SaysNothingForAHostThatHasNoHelperAtAll()
    {
        Assert.False(Decide(presence: IndexerPresence.NotApplicable).Show);
    }

    [Fact]
    public void SaysNothingOnceDismissed()
    {
        Assert.False(Decide(dismissed: true).Show);
    }

    /// <summary>
    /// <b>The anti-double-messaging rule.</b> A failure already appears in the status bar with its
    /// own retry link; the banner saying it too reads as two problems instead of one.
    /// </summary>
    [Fact]
    public void LeavesFailuresToTheStatusBar()
    {
        Assert.False(Decide(failure: "Search index off — permission declined.").Show);
    }

    /// <summary>
    /// A standard user cannot elevate, so this would be an offer that could never be taken up,
    /// repeated on every single launch. The status bar says it once instead.
    /// </summary>
    [Fact]
    public void NeverNagsAUserWhoCannotElevate()
    {
        Assert.False(Decide(canStart: false).Show);
    }

    [Fact]
    public void ABannerAndAFailureAreNeverBothSpeaking()
    {
        foreach (var presence in Enum.GetValues<IndexerPresence>())
        {
            foreach (var canStart in new[] { true, false })
            {
                var withFailure = IndexerBannerRules.Decide(presence, canStart, false, "something went wrong");
                Assert.False(withFailure.Show);
            }
        }
    }

    [Fact]
    public void AHiddenBannerCarriesNoMessageToRender()
    {
        var banner = Decide(presence: IndexerPresence.Running);

        Assert.Equal("", banner.Message);
        Assert.False(banner.CanStart);
    }
}
