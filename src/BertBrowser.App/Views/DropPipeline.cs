using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>
/// The drop half of a drag: deciding whether a destination will take the payload, highlighting it,
/// and carrying the drop out. One of these exists per drop target — every pane's file list, plus
/// the folder tree — because each has its own hover state and its own place to report to.
/// </summary>
/// <remarks>
/// The plan shown while hovering is cached per target, but the drop <em>re-plans from scratch</em>
/// before anything is written. The hover plan decides only whether the cursor says "allowed"; it is
/// never the plan that gets executed.
/// </remarks>
internal sealed class DropPipeline(ShellViewModel shell, Action<string> report)
{
    /// <summary>
    /// Private clipboard format, and the <em>only</em> one this pipeline reads.
    /// </summary>
    /// <remarks>
    /// A drag out of this app also carries <see cref="DataFormats.FileDrop"/> so other applications
    /// can take it — but nothing here looks at that, deliberately. Reading only the private format
    /// means an in-app drop is decided by exactly the same code and the same plan it always was,
    /// and it is what keeps drops arriving <em>from</em> other applications out of scope rather
    /// than half-supported.
    /// </remarks>
    public const string ItemsFormat = "BertBrowser.FileItems";

    private DependencyObject? _highlighted;

    // Hover plan cache: the sources are fixed for the duration of a drag, so only the target and
    // the verb can change.
    private string? _cachedTarget;
    private TransferVerb _cachedVerb;
    private bool _cachedAllowed;

    public void HandleDragOver(DragEventArgs e, string? destination, DependencyObject? highlight)
    {
        e.Handled = true;
        e.Effects = DragDropEffects.None;

        if (Sources(e) is not { Length: > 0 } sources || destination is null || shell.IsTransferring)
        {
            ClearHighlight();
            return;
        }

        var verb = VerbFor(e.KeyStates);
        if (!IsAllowed(sources, destination, verb))
        {
            ClearHighlight();
            return;
        }

        e.Effects = verb == TransferVerb.Copy ? DragDropEffects.Copy : DragDropEffects.Move;
        SetHighlight(highlight);
    }

    /// <summary>Hover-time answer only: whether the cursor should say "you can drop here". The plan
    /// behind it is never the one executed.</summary>
    private bool IsAllowed(string[] sources, string destination, TransferVerb verb)
    {
        if (_cachedTarget == destination && _cachedVerb == verb) return _cachedAllowed;

        _cachedTarget = destination;
        _cachedVerb = verb;
        _cachedAllowed = shell.PlanDrop(sources, destination, verb).HasWork;
        return _cachedAllowed;
    }

    public void InvalidateHoverCache()
    {
        _cachedTarget = null;
        _cachedAllowed = false;
    }

    public async void HandleDrop(DragEventArgs e, string? destination)
    {
        e.Handled = true;
        ClearHighlight();
        InvalidateHoverCache();

        if (Sources(e) is not { Length: > 0 } sources || destination is null) return;

        // This drop is ours, so the drag source must not also act on it. Both of the next two lines
        // say so independently: the session flag, and an effect of None for DoDragDrop to return.
        // Without them, our own move would be read as an external one and the items we just placed
        // would be deleted from where they came from — which they have already left.
        DragSession.ClaimInApp();
        e.Effects = DragDropEffects.None;

        var verb = VerbFor(e.KeyStates);

        try
        {
            // Re-planned here against live disk state: the hover plan was only ever advisory.
            var plan = shell.PlanDrop(sources, destination, verb);

            if (!plan.HasWork)
            {
                report(plan.Problems.Count > 0
                    ? plan.Problems[0].Message
                    : "Nothing to move — those items are already there.");
                return;
            }

            IReadOnlyDictionary<string, ConflictResolution>? resolutions = null;
            if (plan.Conflicts.Count > 0)
            {
                if (ConflictPrompt.Ask(plan) is not { } resolution)
                {
                    report("Drop cancelled.");
                    return;
                }
                resolutions = plan.Transfers.ToDictionary(
                    t => BertBrowser.Core.Paths.PathKey.Canonicalize(t.SourcePath), _ => resolution);
            }

            await shell.ExecuteDropAsync(plan, resolutions);
        }
        catch (Exception ex)
        {
            // An unhandled exception in an async void handler would take the process down.
            report($"Drop failed: {ex.Message}");
        }
    }

    public static string[]? Sources(DragEventArgs e) =>
        e.Data.GetDataPresent(ItemsFormat) ? e.Data.GetData(ItemsFormat) as string[] : null;

    private static TransferVerb VerbFor(DragDropKeyStates keys) =>
        (keys & DragDropKeyStates.ControlKey) != 0 ? TransferVerb.Copy : TransferVerb.Move;

    // --- Drop-target highlight ---

    private void SetHighlight(DependencyObject? target)
    {
        if (ReferenceEquals(_highlighted, target)) return;
        ClearHighlight();
        if (target is null) return;
        DropTarget.SetIsActive(target, true);
        _highlighted = target;
    }

    /// <summary>WPF raises DragLeave on the old target before DragOver on the new one, and each
    /// pipeline only ever clears its own highlight — so dragging from one pane's list to another's,
    /// or onto the tree, never leaves a row lit up behind.</summary>
    public void ClearHighlight()
    {
        if (_highlighted is null) return;
        DropTarget.SetIsActive(_highlighted, false);
        _highlighted = null;
    }
}

/// <summary>Marks the container currently under a drag, so the row styles can highlight it.</summary>
public static class DropTarget
{
    public static readonly DependencyProperty IsActiveProperty = DependencyProperty.RegisterAttached(
        "IsActive", typeof(bool), typeof(DropTarget), new PropertyMetadata(false));

    public static void SetIsActive(DependencyObject element, bool value) =>
        element.SetValue(IsActiveProperty, value);

    public static bool GetIsActive(DependencyObject element) =>
        (bool)element.GetValue(IsActiveProperty);
}
