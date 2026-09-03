namespace BertBrowser.Core.Services.Changes;

/// <summary>
/// Why the timeline cannot show anything, when it cannot. Each is a different thing having gone
/// wrong and gets its own wording, for the reason <c>DiskUsageAvailability</c> has: a feature that
/// is switched off must not look like a quiet disk.
/// </summary>
public enum ChangeTimelineAvailability
{
    Ready,

    /// <summary>The user has not turned recording on. Nothing is written, so nothing can be shown.</summary>
    RecordingOff,

    /// <summary>The volume's initial index is still building; recording starts when it completes.</summary>
    Building,

    /// <summary>The index helper declined, died, or found nothing to index.</summary>
    IndexerUnavailable,

    /// <summary>The folder asked about sits on a drive the helper does not cover.</summary>
    ScopeNotIndexed,
}
