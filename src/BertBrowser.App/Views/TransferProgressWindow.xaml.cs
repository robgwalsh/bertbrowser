using System.Windows;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>
/// The detail view of the queue: every item of the job in flight, the overall bar, throughput, time
/// remaining, the way out — and everything waiting behind it.
/// </summary>
/// <remarks>
/// <para>
/// It binds to the very same <see cref="TransferQueueViewModel"/> the status-bar strip does, whose
/// <c>Running</c> is the very same <see cref="TransferProgressViewModel"/> — so there is one source
/// of truth and no two surfaces can drift apart.
/// </para>
/// <para>
/// <b>Modeless, and closing it does not cancel.</b> Unlike <see cref="DeleteDialog"/> — where
/// closing abandons a survey nothing depends on — the transfer here outlives the window, and
/// hiding a progress view is not a request to stop moving files. Only Cancel cancels.
/// </para>
/// </remarks>
public partial class TransferProgressWindow : ThemedWindow
{
    private TransferProgressWindow(TransferQueueViewModel queue)
    {
        InitializeComponent();
        DataContext = queue;
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static TransferProgressWindow Create(TransferQueueViewModel queue) => new(queue);

    /// <summary>Opens it over <paramref name="owner"/>, modelessly.</summary>
    public static TransferProgressWindow Show(Window? owner, TransferQueueViewModel queue)
    {
        var window = new TransferProgressWindow(queue);
        if (owner is not null && !ReferenceEquals(owner, window)) window.Owner = owner;
        window.Show();
        return window;
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();
}
