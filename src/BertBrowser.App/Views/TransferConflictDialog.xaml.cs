using System.Windows;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>Asks how to settle destination names that are already taken. Each clash carries its own
/// answer, read back off the view model as a per-path map; closing or cancelling abandons the whole
/// transfer rather than doing part of it.</summary>
public partial class TransferConflictDialog : ThemedWindow
{
    public TransferConflictDialog(TransferConflictsViewModel vm)
    {
        InitializeComponent();
        DataContext = vm;
    }

    /// <summary>The harness's way in, so a capture is of the same window a transfer opens.</summary>
    internal static TransferConflictDialog Create(TransferConflictsViewModel vm) => new(vm);

    private void Continue_Click(object sender, RoutedEventArgs e) => DialogResult = true;
}
