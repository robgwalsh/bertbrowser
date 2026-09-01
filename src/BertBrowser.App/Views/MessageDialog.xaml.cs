using System.Windows;
using System.Windows.Media;
using BertBrowser.Core.Theming;

namespace BertBrowser.App.Views;

/// <summary>
/// A themed stand-in for <see cref="MessageBox"/>, which the OS draws and therefore cannot be
/// themed. Mirrors the call shape of <c>MessageBox.Show</c> so switching a call site is one line.
/// </summary>
public partial class MessageDialog : ThemedWindow
{
    private MessageDialog()
    {
        InitializeComponent();
    }

    public static bool Show(
        Window? owner,
        string message,
        string caption,
        MessageDialogKind kind = MessageDialogKind.Information,
        bool showCancel = false)
    {
        var dialog = Create(message, caption, kind, showCancel);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true;
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. <see cref="Show"/> goes through it too, so a capture cannot drift from what
    /// the app actually puts on screen.</summary>
    internal static MessageDialog Create(
        string message,
        string caption,
        MessageDialogKind kind = MessageDialogKind.Information,
        bool showCancel = false)
    {
        var dialog = new MessageDialog { Title = caption };

        dialog.MessageText.Text = message;
        dialog.CancelButton.Visibility = showCancel ? Visibility.Visible : Visibility.Collapsed;

        var (icon, token) = kind switch
        {
            MessageDialogKind.Warning => ("Icon.Warning", ThemeToken.WarningForeground),
            MessageDialogKind.Error => ("Icon.Error", ThemeToken.ErrorForeground),
            _ => ("Icon.Info", ThemeToken.TextLink),
        };
        dialog.Glyph.Data = (Geometry)dialog.FindResource(icon);
        if (dialog.TryFindResource(token) is Brush brush) dialog.Glyph.Fill = brush;

        return dialog;
    }

    private void Ok_Click(object sender, RoutedEventArgs e) => DialogResult = true;
}

public enum MessageDialogKind
{
    Information,
    Warning,
    Error,
}
