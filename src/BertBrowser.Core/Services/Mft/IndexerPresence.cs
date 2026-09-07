namespace BertBrowser.Core.Services.Mft;

/// <summary>
/// Whether there is an elevated index helper to talk to.
/// </summary>
/// <remarks>
/// <para>
/// Three values rather than a bool, and the third is the point. Only <c>MftIndexClient</c> is a
/// client of a separate helper process; the in-process <see cref="MftIndexService"/> and
/// <c>NullMftIndexService</c> are not, and a bool would force them both to answer "not running" —
/// which would raise the "start the indexer" banner in every harness run and every screenshot, over
/// a helper those hosts were never going to have.
/// </para>
/// <para>
/// <see cref="NotApplicable"/> is the honest answer for them, in the same spirit
/// <c>NullMftIndexService</c> already answers honestly rather than conveniently.
/// </para>
/// </remarks>
public enum IndexerPresence
{
    /// <summary>This host does not use a separate helper, so the question does not arise.</summary>
    NotApplicable,

    /// <summary>No helper is running. The user may be offered one.</summary>
    NotRunning,

    /// <summary>A helper is running, whether or not this process has attached to it yet.</summary>
    Running,
}
