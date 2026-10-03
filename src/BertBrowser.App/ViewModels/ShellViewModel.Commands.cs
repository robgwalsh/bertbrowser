using BertBrowser.Core.Layout;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.ViewModels;

/// <summary>
/// The shell's half of the commands that had no button to hang off before there was a palette:
/// swapping panes, sending a tab or a selection to the other one, and the few transfers that are
/// not a drop or a paste. Each is a thin arrangement of something the shell already does.
/// </summary>
public sealed partial class ShellViewModel
{
    /// <summary>
    /// The pane "the other pane" means from here: the one that is not active when there are two,
    /// and the next one in reading order when there are more. Null with a single pane.
    /// </summary>
    /// <remarks>
    /// Deliberately looser than <see cref="ComparePair"/>, which refuses anything but exactly two.
    /// A comparison paints two panes and offers to delete from one, so guessing the pair is not
    /// acceptable; sending a folder next door is something F6 already defines an answer for.
    /// </remarks>
    public PaneViewModel? OtherPane
    {
        get
        {
            if (LayoutTree.FindLeaf(Layout, ActivePane) is not { } leaf) return null;
            var next = LayoutTree.NextLeaf(Layout, leaf, 1).Value;
            return ReferenceEquals(next, ActivePane) ? null : next;
        }
    }

    /// <summary>Exchanges the active pane with the other one. The panes move; which one is active
    /// does not, so the keyboard stays with the folder it was in.</summary>
    public void SwapPanes()
    {
        // Never under a comparison: see the command's own reason. Guarded here too, since this is
        // the method and the command is only one way to reach it.
        if (CompareSession is not null || OtherPane is not { } other) return;
        if (LayoutTree.FindLeaf(Layout, ActivePane) is not { } here ||
            LayoutTree.FindLeaf(Layout, other) is not { } there) return;

        Layout = LayoutTree.Swap(Layout, here, there);
        LayoutChanged?.Invoke();
        PaneFocusRequested?.Invoke(ActivePane);
    }

    /// <summary>Puts every splitter back to an even share.</summary>
    public void EqualisePanes()
    {
        LayoutTree.Equalise(Layout);
        LayoutChanged?.Invoke();
    }

    /// <summary>
    /// Moves the active tab into the other pane, carrying the tab itself across — not a clone — so
    /// its history and selection survive, the way <see cref="MoveTabToNewPane"/> does. A pane left
    /// with no tabs closes, exactly as it would on closing its last one.
    /// </summary>
    public void MoveActiveTabToOtherPane()
    {
        if (CompareSession is not null || OtherPane is not { } target || ActivePane.ActiveTab is not { } tab) return;
        var source = ActivePane;

        source.DetachTab(tab);
        target.AdoptTab(tab);
        ActivatePane(target);

        if (source.Tabs.Count == 0) ClosePane(source);
        PaneFocusRequested?.Invoke(target);
    }

    /// <summary>Shows <paramref name="path"/> in the other pane, or in a new one beside this when
    /// there is no other pane yet. Focus stays where it is: the point is to look at two folders,
    /// and the one being worked in has not changed.</summary>
    public void OpenInOtherPane(string path)
    {
        if (path.Length == 0) return;

        if (OtherPane?.ActiveTab is { } tab)
        {
            _ = tab.NavigateToAsync(path);
            return;
        }

        var here = ActivePane;
        SplitPane(here, SplitOrientation.Vertical, path);
        ActivatePane(here);
        PaneFocusRequested?.Invoke(here);
    }

    /// <summary>
    /// Copies or moves <paramref name="sources"/> into <paramref name="destination"/>.
    /// </summary>
    /// <remarks>
    /// A drop by another name, as paste is: it goes through the one planner and executor that
    /// relocate user data, so it gets the queue, byte progress, the conflict dialog, a merged
    /// folder and undo without any of it being written a second time.
    /// </remarks>
    public async Task TransferToAsync(IReadOnlyList<string> sources, string destination, TransferVerb verb)
    {
        if (sources.Count == 0 || destination.Length == 0) return;

        var plan = PlanDrop(sources, destination, verb);
        if (!plan.HasWork)
        {
            var doing = verb == TransferVerb.Move ? "moved" : "copied";
            SetStatus(plan.Problems.Count > 0
                ? $"Nothing {doing} — {plan.Problems[0].Message}"
                : $"Nothing to be {doing} there");
            return;
        }

        await ExecuteDropAsync(plan, resolutions: null);
    }

    /// <summary>Writes a shortcut to each of <paramref name="sources"/> into
    /// <paramref name="directory"/>, as the right-drag menu's third verb does.</summary>
    public async Task CreateShortcutsInAsync(IReadOnlyList<string> sources, string directory)
    {
        if (sources.Count == 0 || directory.Length == 0) return;

        var plan = PlanShortcuts(sources, directory);
        if (!plan.HasWork)
        {
            SetStatus(plan.Problems.Count > 0
                ? $"No shortcut created — {plan.Problems[0].Message}"
                : "Nothing to make a shortcut to");
            return;
        }

        await CreateShortcutsAsync(plan);
    }
}
