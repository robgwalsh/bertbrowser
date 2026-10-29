using System.Windows;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks for an encrypted archive's password.
/// </summary>
/// <remarks>
/// <para>
/// Raised from the banner's Unlock button, never from the background load that discovered the
/// archive was locked — a modal from a worker thread is not a modal, and it would have to be raised
/// again on every back, forward and refresh.
/// </para>
/// <para>
/// <b>Nothing typed here is persisted</b>, and the reason is not vagueness about cryptography:
/// <c>settings.json</c> and <c>bertbrowser.db</c> are both plain files in the profile, so
/// "remembered" would mean "written in the clear beside the archive it unlocks". A file browser
/// that silently remembers archive passwords is a credential store, and this app is not one.
/// </para>
/// </remarks>
public partial class ArchivePasswordDialog : ThemedWindow
{
    private ArchivePasswordDialog(string archiveName, bool retry)
    {
        InitializeComponent();

        PromptText.Text = $"{archiveName} is protected. Enter its password to read it.";

        if (retry)
        {
            ProblemText.Text = "That password did not work.";
            ProblemText.Visibility = Visibility.Visible;
        }

        Loaded += (_, _) => PasswordEntry.Focus();
    }

    /// <summary>The dialog built but not shown, for the UI harness to photograph.</summary>
    internal static ArchivePasswordDialog Create(Window? owner, string archiveName, bool retry = false) =>
        new(archiveName, retry) { Owner = owner };

    public string Password => PasswordEntry.Password;

    private void Password_Changed(object sender, RoutedEventArgs e) =>
        OkButton.IsEnabled = PasswordEntry.Password.Length > 0;

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        if (PasswordEntry.Password.Length == 0) return;
        DialogResult = true;
    }
}
