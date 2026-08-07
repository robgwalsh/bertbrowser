using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>Asks how to settle destination names that are already taken. The chosen
/// <see cref="ConflictResolution"/> applies to every conflicting item in the drop; closing or
/// cancelling leaves <see cref="Resolution"/> null and the drop is abandoned entirely.</summary>
public partial class TransferConflictDialog : Window
{
    /// <summary>Null when the user cancelled.</summary>
    public ConflictResolution? Resolution { get; private set; }

    public TransferConflictDialog(TransferConflictsViewModel vm)
    {
        InitializeComponent();
        DataContext = vm;
    }

    private void KeepBoth_Click(object sender, RoutedEventArgs e) => Close(ConflictResolution.KeepBoth);

    private void Skip_Click(object sender, RoutedEventArgs e) => Close(ConflictResolution.Skip);

    private void Replace_Click(object sender, RoutedEventArgs e) => Close(ConflictResolution.Replace);

    private void Close(ConflictResolution resolution)
    {
        Resolution = resolution;
        DialogResult = true;
    }
}
