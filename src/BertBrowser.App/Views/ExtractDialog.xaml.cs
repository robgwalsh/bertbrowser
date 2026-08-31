using System.IO;
using System.Windows;
using BertBrowser.Core.Services.Archives;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks where an archive's contents should go, and what to do about names already there.
/// </summary>
/// <remarks>
/// <para>
/// The destination box is seeded with what "Extract here" would have done —
/// <see cref="ExtractRules.DestinationFor"/> — so opening this and pressing Extract does the same
/// thing as not opening it at all. One click never changes the pending result, which is the rule
/// the rename dialog's options panel follows for the same reason.
/// </para>
/// <para>
/// <b>There is no "replace" option, and its absence is the design.</b> With only Skip and Keep
/// both, extracting is purely additive: nothing needs an undo record, nothing is staged, and no
/// existing file can be lost. Overwriting the destination is the single commonest way people lose
/// work to an unzip, and it is not something to put one keystroke away.
/// </para>
/// </remarks>
public partial class ExtractDialog : ThemedWindow
{
    private ExtractDialog(string archiveName, string destination, int selectedCount)
    {
        InitializeComponent();

        PromptText.Text = selectedCount > 0
            ? $"Extract {selectedCount:N0} selected item(s) from {archiveName}."
            : $"Extract everything from {archiveName}.";

        DestinationBox.Text = destination;
        Validate();

        Loaded += (_, _) =>
        {
            DestinationBox.Focus();
            DestinationBox.SelectAll();
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to photograph. The app reaches
    /// it through the same constructor.</summary>
    internal static ExtractDialog Create(
        Window? owner, string archiveName, string destination, int selectedCount) =>
        new(archiveName, destination, selectedCount) { Owner = owner };

    public string Destination => DestinationBox.Text.Trim();

    public ExtractConflict Conflict =>
        SkipOption.IsChecked == true ? ExtractConflict.Skip : ExtractConflict.KeepBoth;

    private void DestinationBox_TextChanged(object sender, System.Windows.Controls.TextChangedEventArgs e) =>
        Validate();

    /// <summary>
    /// Only the things that can be judged from the text alone. Everything else — whether the
    /// container still reads, whether the destination is inside it — is the planner's, and is
    /// re-checked against live disk when Extract is pressed.
    /// </summary>
    private void Validate()
    {
        var path = Destination;
        string? problem = null;

        if (path.Length == 0) problem = "Choose a folder to extract into.";
        else if (!Path.IsPathFullyQualified(path)) problem = "That is not a full path.";
        else if (File.Exists(path)) problem = "That is a file, not a folder.";

        ProblemText.Text = problem ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;
        OkButton.IsEnabled = problem is null;
    }

    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        var picker = new Microsoft.Win32.OpenFolderDialog
        {
            Title = "Extract to",
            InitialDirectory = Directory.Exists(Destination)
                ? Destination
                : Path.GetDirectoryName(Destination) ?? "",
        };

        if (picker.ShowDialog(this) == true) DestinationBox.Text = picker.FolderName;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        Validate();
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
