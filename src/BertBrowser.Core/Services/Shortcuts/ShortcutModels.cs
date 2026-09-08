namespace BertBrowser.Core.Services.Shortcuts;

/// <param name="TargetPath">What the shortcut points at.</param>
/// <param name="LinkPath">The <c>.lnk</c> to write, name already stepped aside from anything in the
/// way — including the other links in the same plan.</param>
public sealed record ShortcutCreation(string TargetPath, string LinkPath);

/// <param name="SourcePath">The item no shortcut can be made for. Empty when the destination itself
/// is the problem, since then no one source is at fault.</param>
/// <param name="Message">The refusal, phrased for the status bar.</param>
public sealed record RejectedShortcut(string SourcePath, string Message);

/// <summary>What creating shortcuts for a selection would produce, decided without writing
/// anything.</summary>
/// <param name="Directory">The folder the links go in.</param>
public sealed record ShortcutPlan(
    string Directory,
    IReadOnlyList<ShortcutCreation> Creations,
    IReadOnlyList<RejectedShortcut> Problems)
{
    public bool HasWork => Creations.Count > 0;

    public static ShortcutPlan Empty { get; } = new("", [], []);
}

/// <param name="Message">The failure, phrased for the status bar.</param>
/// <param name="AccessDenied">Windows refused permission, rather than the path being unusable. The
/// one failure an administrator token could fix — not offered here yet, but worth carrying so the
/// message can say which kind of failure it was.</param>
public sealed record FailedShortcut(string LinkPath, string Message, bool AccessDenied = false);

/// <summary>What actually got written.</summary>
/// <remarks>
/// There is deliberately no undo record, for the reason <see cref="NewItem.NewItemOutcome"/> gives:
/// creating is additive, exactly as copying is, and nothing existing is touched. Ctrl+Z is left
/// pointing at whatever move, rename or delete came before — which is the more valuable thing to
/// have on the one undo slot than a handful of links the user can select and delete.
/// </remarks>
public sealed record ShortcutOutcome(
    IReadOnlyList<string> Created,
    IReadOnlyList<FailedShortcut> Failed)
{
    public static ShortcutOutcome Empty { get; } = new([], []);
}
