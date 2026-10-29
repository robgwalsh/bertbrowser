using System.Windows;
using System.Windows.Input;

namespace BertBrowser.App.Views;

/// <summary>
/// The search query language, on one page, behind the info button beside the whole-PC field.
/// </summary>
/// <remarks>
/// A dialog rather than the tooltip that used to carry this: the filter list is now long enough
/// that a tooltip is the wrong shape for it — it cannot be scrolled, it cannot be kept open while
/// the query is typed, and it vanishes the moment the pointer moves. The tooltip stays on the
/// per-tab box as the quick peek; this is the reference.
/// </remarks>
public partial class SearchSyntaxDialog : ThemedWindow
{
    private SearchSyntaxDialog()
    {
        InitializeComponent();
    }

    public static void Show(Window? owner)
    {
        var dialog = Create();
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;
        dialog.ShowDialog();
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. <see cref="Show"/> goes through it too, so a capture cannot drift from what
    /// the app actually puts on screen.</summary>
    internal static SearchSyntaxDialog Create() => new();

    /// <summary>
    /// Any key closes it, including a bare modifier — there is nothing here to type into or
    /// navigate, so every key press is someone getting on with what they were doing.
    /// </summary>
    /// <remarks>
    /// This window has <em>nothing focusable in it</em> — no button since any input closes it, no
    /// scrollbar since it sizes to its content — which is worth knowing before assuming a key
    /// press arrives at all. Measured: WPF gives keyboard focus to the <c>Window</c> itself, so
    /// this fires; and were a focusable control added later, a tunnelling Preview handler on the
    /// window still sees the key first. Either way it holds.
    /// </remarks>
    private void Dismiss_Key(object sender, KeyEventArgs e)
    {
        e.Handled = true;
        Close();
    }

    /// <summary>
    /// So does a click anywhere on the page.
    /// </summary>
    /// <remarks>
    /// The title bar is the exception, and not one worth fighting: <c>WindowChrome</c> declares
    /// the top 32px as non-client, so a press there goes to the window manager as a drag and
    /// never reaches WPF at all. Making it close would mean marking the caption hit-test-visible,
    /// which would cost the drag for every window sharing that chrome. A title bar that behaves
    /// like a title bar — and still carries the close button — is the better trade.
    /// </remarks>
    private void Dismiss_Mouse(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        Close();
    }
}
