using System.Windows;
using BertBrowser.Core.Services;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks it in a themed modal over the main window.
/// </summary>
/// <remarks>
/// <see cref="UserNotice"/>'s sibling, and it exists for the same reason: so a view model can ask a
/// question without knowing what a <c>Window</c> is, and a scripted run can answer without one.
/// </remarks>
public sealed class UserConfirm : IUserConfirm
{
    public bool Ask(string message, string caption, string confirmLabel) =>
        MessageDialog.Show(
            Application.Current?.MainWindow, message, caption,
            MessageDialogKind.Warning, showCancel: true, confirmLabel: confirmLabel);
}
