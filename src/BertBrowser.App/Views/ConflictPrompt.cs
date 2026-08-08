using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>Asks what to do when a drop would land on names that are already taken. Static rather
/// than a delegate threaded through the pane hierarchy, because every pane's drop controller needs
/// it and they all want the same window-owned dialog.</summary>
internal static class ConflictPrompt
{
    /// <summary>Returns null when the user cancels, which abandons the whole drop.</summary>
    public static ConflictResolution? Ask(TransferPlan plan)
    {
        var dialog = new TransferConflictDialog(new TransferConflictsViewModel(plan))
        {
            Owner = Application.Current?.MainWindow,
        };
        return dialog.ShowDialog() == true ? dialog.Resolution : null;
    }
}
