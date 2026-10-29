namespace BertBrowser.Core.Services.Mft;

/// <summary>What, if anything, the window should say about a missing index helper.</summary>
/// <param name="Show">Whether the banner appears at all.</param>
/// <param name="Message">What it says. Empty when it does not appear.</param>
/// <param name="CanStart">Whether to offer the button that raises the elevation prompt.</param>
public readonly record struct IndexerBanner(bool Show, string Message, bool CanStart)
{
    public static readonly IndexerBanner Hidden = new(false, "", false);
}

/// <summary>
/// When to offer to start the index helper, and when to say nothing.
/// </summary>
/// <remarks>
/// <para>
/// The app used to raise an elevation prompt on every launch, before anyone had asked for anything.
/// This is what replaced it: a strip at the bottom of the window that explains what is missing and
/// offers to fetch it. So the interesting cases here are all the ones where it must
/// <em>not</em> appear — a banner nobody can act on, or one repeating what the status bar already
/// says, is worse than the prompt it replaced.
/// </para>
/// <para>Pure, so every row of that table is a test rather than something to notice in a screenshot.</para>
/// </remarks>
public static class IndexerBannerRules
{
    /// <summary>
    /// What the ordinary "no helper yet" banner says. It has to answer three questions at once —
    /// what is missing, what it costs, and how often it will cost it — because someone who does not
    /// know why an app wants administrator rights should not have to guess.
    /// </summary>
    public const string Offer =
        "Whole-PC search and folder sizes need the index helper. " +
        "It asks for administrator rights once and keeps running until you sign out.";

    /// <param name="canStart">
    /// Whether asking for a helper could actually produce one — false for an account that cannot
    /// elevate, and for a second copy of the app that will never own the endpoint.
    /// </param>
    public static IndexerBanner Decide(
        IndexerPresence presence,
        bool canStart,
        bool dismissed,
        string failure)
    {
        // Asked to go away for this session.
        if (dismissed) return IndexerBanner.Hidden;

        // This host has no separate helper — the in-process indexer, or the UI harness. Saying one
        // is missing would put the banner over every scripted screenshot, about a process those
        // hosts were never going to have.
        if (presence == IndexerPresence.NotApplicable) return IndexerBanner.Hidden;

        if (presence == IndexerPresence.Running) return IndexerBanner.Hidden;

        // Something went wrong and the status bar is already saying so, with its own retry link.
        // Two places describing one problem reads as two problems.
        if (failure.Length > 0) return IndexerBanner.Hidden;

        // Nothing to offer: a standard user cannot elevate, and a banner they can do nothing about
        // on every single launch is nagging. The status bar says it once, quietly.
        if (!canStart) return IndexerBanner.Hidden;

        return new IndexerBanner(true, Offer, CanStart: true);
    }
}
