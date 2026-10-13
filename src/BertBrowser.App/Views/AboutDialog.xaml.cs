using System.Windows;
using BertBrowser.App.Services;

namespace BertBrowser.App.Views;

/// <summary>
/// What this copy is: its name, its version, and the release channel it came from. The title bar
/// used to carry the version; the menu bar has that place now, so Help ▸ About is where it reads.
/// </summary>
public partial class AboutDialog : ThemedWindow
{
    private AboutDialog()
    {
        InitializeComponent();

        Logo.Source = AppIcon.ForSlot(48, 1.0);
        VersionText.Text = AppVersion.Display.Length > 0 ? $"Version {AppVersion.Display}" : "Version unknown";
        ChannelText.Text = AppVersion.IsUnstable ? "Unstable channel" : "Stable channel";
    }

    public static void Show(Window? owner)
    {
        var dialog = Create();
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;
        dialog.ShowDialog();
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. <see cref="Show"/> goes through it too.</summary>
    internal static AboutDialog Create() => new();
}
