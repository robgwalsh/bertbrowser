using System.Windows;
using BertBrowser.Core.Services;

namespace BertBrowser.App.Views;

/// <summary>
/// Says it in a themed modal over the main window.
/// </summary>
/// <remarks>
/// A tiny class, and it exists so <c>ShellViewModel</c> can tell the user something without knowing
/// what a <c>Window</c> is — exactly as <c>ElevationPrompt</c> does for a question. That is what
/// lets the harness host the same window offscreen and record the message rather than opening a
/// modal a script would then wait on for ever.
/// </remarks>
public sealed class UserNotice : IUserNotice
{
    public void Say(string message, string caption) =>
        MessageDialog.Show(
            Application.Current?.MainWindow, message, caption, MessageDialogKind.Information);
}
