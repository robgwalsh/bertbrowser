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
        var dialog = new MessageDialog { Title = caption };
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        dialog.MessageText.Text = message;
        dialog.CancelButton.Visibility = showCancel ? Visibility.Visible : Visibility.Collapsed;

        // Segoe Fluent Icons codepoints: Warning, Error, Info.
        var (glyph, token) = kind switch
        {
            MessageDialogKind.Warning => (0xE7BA, ThemeToken.WarningForeground),
            MessageDialogKind.Error => (0xE783, ThemeToken.ErrorForeground),
            _ => (0xE946, ThemeToken.TextLink),
        };
        dialog.Glyph.Text = char.ConvertFromUtf32(glyph);
        if (dialog.TryFindResource(token) is Brush brush) dialog.Glyph.Foreground = brush;

        return dialog.ShowDialog() == true;
    }

    private void Ok_Click(object sender, RoutedEventArgs e) => DialogResult = true;
}

public enum MessageDialogKind
{
    Information,
    Warning,
    Error,
}
