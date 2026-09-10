using BertBrowser.Core.Paths;

namespace BertBrowser.Core.Services.Transfer;

/// <summary>
/// Asking how to settle destination names that are already taken.
/// </summary>
/// <remarks>
/// <para>
/// A seam for the reason <see cref="IUserConfirm"/> is one: a modal raised from inside the shell
/// would block the scripted run that hosts the same window offscreen, and a test has nobody to ask.
/// It is also what lets <em>every</em> way of transferring reach the same dialog — the question used
/// to live in the drop pipeline, which is why pasting onto a name that already existed quietly
/// numbered the newcomer and never asked at all.
/// </para>
/// <para>
/// The answer is per path, because <c>TransferExecutor.Execute</c> has taken a per-path map since it
/// was written. Only the clashing items need an entry; anything unlisted falls back to
/// <see cref="ConflictResolution.KeepBoth"/>, which can destroy nothing.
/// </para>
/// </remarks>
public interface IConflictPrompt
{
    /// <summary>
    /// What to do with each clash in <paramref name="plan"/>, keyed by
    /// <see cref="PathKey.Canonicalize"/> of the source path — or null when the user cancelled,
    /// which abandons the whole transfer rather than doing part of it.
    /// </summary>
    IReadOnlyDictionary<string, ConflictResolution>? Ask(TransferPlan plan);
}

/// <summary>
/// Answers without asking. What a context with nobody to ask gets.
/// </summary>
/// <remarks>
/// Keep both rather than cancel, because this is not a question about whether to proceed: the user
/// already asked for the transfer, and the one resolution that neither loses the incoming copy nor
/// displaces the existing one is the answer that assumes least. It is also what every unattended
/// path did before there was a prompt at all. The one exception is a clash with a byte-identical
/// file, which <see cref="ConflictDefaults"/> settles as Skip — through the same call the dialog
/// seeds its rows from, so the unattended answer and the offered one cannot drift.
/// </remarks>
public sealed class KeepBothConflictPrompt : IConflictPrompt
{
    public IReadOnlyDictionary<string, ConflictResolution>? Ask(TransferPlan plan) =>
        plan.Conflicts.ToDictionary(
            t => PathKey.Canonicalize(t.SourcePath),
            ConflictDefaults.For);
}
