using System.Collections.Generic;
using System.Windows;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Transfer;

namespace BertBrowser.App.Views;

/// <summary>Asks what to do when a transfer would land on names that are already taken, in a themed
/// modal over the main window. <see cref="UserConfirm"/>'s sibling, and a seam for the same
/// reason.</summary>
public sealed class ConflictPrompt : IConflictPrompt
{
    /// <summary>Returns null when the user cancels, which abandons the whole transfer.</summary>
    public IReadOnlyDictionary<string, ConflictResolution>? Ask(TransferPlan plan)
    {
        var view = new TransferConflictsViewModel(plan);
        var dialog = new TransferConflictDialog(view)
        {
            Owner = Application.Current?.MainWindow,
        };
        return dialog.ShowDialog() == true ? view.Resolutions : null;
    }
}
