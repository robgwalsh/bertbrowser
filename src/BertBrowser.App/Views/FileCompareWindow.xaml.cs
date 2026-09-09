using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// "Are these two files the same, and if not, where?" — a byte verdict for any pair, a side-by-side
/// line diff for two text files, and a hex dump around the first differing byte for anything else.
/// </summary>
/// <remarks>
/// Modeless: it outlives a byte comparison of two disc images, and it is read while browsing the
/// folders the two files came from.
/// </remarks>
public partial class FileCompareWindow : ThemedWindow
{
    /// <remarks>Not disposed here — whoever built it owns it, as with the other tool windows.</remarks>
    private readonly FileCompareViewModel _vm;

    public FileCompareWindow(FileCompareViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;

        // The view model decides which row to go to; putting it on screen is the view's job, and
        // this is the whole of the seam between them.
        vm.ScrollRequested += ScrollTo;

        Loaded += async (_, _) => await vm.CompareAsync();
        Closed += (_, _) => vm.ScrollRequested -= ScrollTo;
    }

    /// <summary>The harness photographs this window without ever showing it, through the same
    /// constructor, so a capture cannot drift from what the app puts on screen.</summary>
    internal static FileCompareWindow Create(FileCompareViewModel vm) => new(vm);

    private void ScrollTo(int index)
    {
        if (index < 0 || index >= DiffRows.Items.Count) return;

        DiffRows.ScrollIntoView(DiffRows.Items[index]);
    }
}
