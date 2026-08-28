using System.Windows;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// The detail view of a running transfer: every item, the overall bar, throughput, time remaining,
/// and the way out.
/// </summary>
/// <remarks>
/// <para>
/// It binds to the very same <see cref="TransferProgressViewModel"/> the status-bar strip does, so
/// there is one source of truth and the two surfaces cannot drift apart.
/// </para>
/// <para>
/// <b>Modeless, and closing it does not cancel.</b> Unlike <see cref="DeleteDialog"/> — where
/// closing abandons a survey nothing depends on — the transfer here outlives the window, and
/// hiding a progress view is not a request to stop moving files. Only Cancel cancels.
/// </para>
/// </remarks>
public partial class TransferProgressWindow : ThemedWindow
{
    private TransferProgressWindow(TransferProgressViewModel vm)
    {
        InitializeComponent();
        DataContext = vm;
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static TransferProgressWindow Create(TransferProgressViewModel vm) => new(vm);

    /// <summary>Opens it over <paramref name="owner"/>, modelessly.</summary>
    public static TransferProgressWindow Show(Window? owner, TransferProgressViewModel vm)
    {
        var window = new TransferProgressWindow(vm);
        if (owner is not null && !ReferenceEquals(owner, window)) window.Owner = owner;
        window.Show();
        return window;
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();
}
