namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// Answers whether an index helper is running, without talking to it.
/// </summary>
/// <remarks>
/// <para>
/// Behind an interface so the client's state machine can be tested — and so the UI harness can
/// answer "no" without touching a kernel object. The real implementation is a named mutex the
/// helper holds for its life (<c>IndexerPresenceLock</c>).
/// </para>
/// <para>
/// <b>It is not the same question as "did anything connect".</b> The difference is the case worth
/// getting right: a helper that is running but cannot be reached must not be reported as absent,
/// because the offer to start another one would raise an elevation prompt and then be refused by
/// the helper's own single-instance guard — a prompt spent on nothing.
/// </para>
/// </remarks>
public interface IIndexerPresence
{
    /// <summary>True when a helper for this user is running.</summary>
    bool IsRunning { get; }
}

/// <summary>Reports that nothing is ever running, for hosts with no helper to find.</summary>
public sealed class NoIndexerPresence : IIndexerPresence
{
    public bool IsRunning => false;
}
