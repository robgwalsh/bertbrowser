using System.IO;
using System.Windows;
using BertBrowser.Core.Services.Elevation;

namespace BertBrowser.App.Views;

/// <summary>
/// "Windows refused these — try again as administrator?"
/// </summary>
/// <remarks>
/// The list is bounded at <see cref="MaxListed"/> with a count of the rest. A confirmation you have
/// to scroll is one you cannot take in, and a selection that was refused wholesale can be thousands
/// of rows long.
/// </remarks>
public partial class ElevationDialog : ThemedWindow
{
    private const int MaxListed = 8;

    private ElevationDialog() => InitializeComponent();

    public static bool Confirm(Window? owner, ElevationOffer offer)
    {
        var dialog = Create(offer);
        if (owner is not null && !ReferenceEquals(owner, dialog)) dialog.Owner = owner;

        return dialog.ShowDialog() == true;
    }

    /// <summary>The dialog built but not shown, for the UI harness to park offscreen and
    /// photograph. <see cref="Confirm"/> goes through it too, so a capture cannot drift from what
    /// the app actually puts on screen.</summary>
    internal static ElevationDialog Create(ElevationOffer offer)
    {
        var dialog = new ElevationDialog();

        dialog.Headline.Text = Heading(offer);
        dialog.Explanation.Text =
            "Windows will ask you to confirm. Everything else in this operation has already been done.";
        dialog.Items.ItemsSource = offer.Items.Take(MaxListed).Select(Describe).ToList();

        if (offer.Items.Count > MaxListed)
        {
            dialog.More.Text = $"and {offer.Items.Count - MaxListed:N0} more";
            dialog.More.Visibility = Visibility.Visible;
        }

        return dialog;
    }

    private static string Heading(ElevationOffer offer)
    {
        var count = offer.Items.Count;
        var items = count == 1 ? "1 item" : $"{count:N0} items";
        var verb = offer.Operation switch
        {
            ElevationOperation.TransferMove => offer.IsUndo ? "moved back" : "moved",
            ElevationOperation.TransferCopy => "copied",
            ElevationOperation.Delete => offer.IsUndo ? "restored" : "deleted",
            ElevationOperation.Rename => offer.IsUndo ? "renamed back" : "renamed",
            ElevationOperation.NewItem => "created",
            ElevationOperation.TransferUndo => "moved back",
            ElevationOperation.DeleteUndo => "restored",
            _ => "changed",
        };

        return $"{items} could not be {verb} — Windows refused permission.";
    }

    /// <summary>Name first, then where it is: the name is what the user recognises, and the folder
    /// is what makes two files of the same name tell apart.</summary>
    private static string Describe(string path)
    {
        var name = Path.GetFileName(path);
        var folder = Path.GetDirectoryName(path);
        return name.Length == 0 || folder is null ? path : $"{name}  —  {folder}";
    }

    private void Ok_Click(object sender, RoutedEventArgs e) => DialogResult = true;
}

/// <summary>
/// The app's answer to <see cref="IElevationPrompt"/>: the dialog above, over the main window.
/// </summary>
/// <remarks>
/// A tiny class, and it exists so <c>ShellViewModel</c> can ask the question without knowing what a
/// <c>Window</c> is — which is what lets the harness answer it without one, and a test answer it
/// without a person.
/// </remarks>
public sealed class ElevationPrompt : IElevationPrompt
{
    public bool Offer(ElevationOffer offer) =>
        ElevationDialog.Confirm(Application.Current?.MainWindow, offer);
}
