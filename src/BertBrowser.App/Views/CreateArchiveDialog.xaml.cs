using System.IO;
using System.Windows;
using System.Windows.Controls;
using BertBrowser.Core.Services.Archives;
using BertBrowser.Core.Services.NewItem;
using BertBrowser.Core.Services.Rename;

namespace BertBrowser.App.Views;

/// <summary>
/// Asks what to call a new archive, what format it should be, and how hard to compress.
/// </summary>
/// <remarks>
/// <para>
/// The name is validated with <see cref="RenamePattern.Validate"/> — the same rule a rename and a
/// New obey — rather than a third copy of "what Windows refuses". Only the suffix is this dialog's
/// own business, and it is appended rather than typed, so the name and the format cannot disagree.
/// </para>
/// <para>
/// <b>7z and RAR are not in the list, and the empty space where they would be is explained by the
/// dialog rather than left to be guessed at.</b> Both are perfectly readable; only writing them is
/// impossible. See <see cref="ArchiveWriteRules.WhyNotWritable"/>.
/// </para>
/// </remarks>
public partial class CreateArchiveDialog : ThemedWindow
{
    private static readonly (CompressionLevel Level, string Label)[] Levels =
    [
        (CompressionLevel.Store, "Store (no compression)"),
        (CompressionLevel.Normal, "Normal"),
        (CompressionLevel.Maximum, "Maximum"),
    ];

    private readonly string _directory;

    private CreateArchiveDialog(string directory, string suggestedName, int itemCount)
    {
        InitializeComponent();
        _directory = directory;

        PromptText.Text = itemCount == 1
            ? "Compress 1 item into a new archive."
            : $"Compress {itemCount:N0} items into a new archive.";

        foreach (var format in ArchiveWriteRules.Formats) FormatBox.Items.Add(format.Label);
        FormatBox.SelectedIndex = 0;

        foreach (var (_, label) in Levels) LevelBox.Items.Add(label);
        LevelBox.SelectedIndex = 1;

        NameBox.Text = suggestedName;
        Update();

        Loaded += (_, _) =>
        {
            NameBox.Focus();
            NameBox.SelectAll();
        };
    }

    /// <summary>The dialog built but not shown, for the UI harness to photograph.</summary>
    internal static CreateArchiveDialog Create(
        Window? owner, string directory, string suggestedName, int itemCount) =>
        new(directory, suggestedName, itemCount) { Owner = owner };

    public ArchiveWriteFormat Format =>
        ArchiveWriteRules.Formats[Math.Max(0, FormatBox.SelectedIndex)].Format;

    public CompressionLevel Level => Levels[Math.Max(0, LevelBox.SelectedIndex)].Level;

    /// <summary>The full path the archive will be written to.</summary>
    public string ArchivePath =>
        Path.Combine(_directory, NameBox.Text.Trim() + ArchiveWriteRules.SuffixFor(Format));

    private void Input_Changed(object sender, RoutedEventArgs e) => Update();

    private void Format_Changed(object sender, SelectionChangedEventArgs e)
    {
        // Only Zip has anything to choose between: tar carries no compression of its own, and
        // gzip and bzip2 take their level from the codec rather than the container.
        if (LevelBox is not null)
            LevelBox.IsEnabled = ArchiveWriteRules.Info(Format).SupportsLevel;
        Update();
    }

    private void Update()
    {
        if (ResultText is null) return;   // during InitializeComponent

        var name = NameBox.Text.Trim();
        var problem = name.Length == 0
            ? "Give the archive a name."
            : RenamePattern.Validate(name);

        if (problem is null && File.Exists(ArchivePath))
            problem = $"{Path.GetFileName(ArchivePath)} is already there.";

        ResultText.Text = problem is null ? $"Creates: {ArchivePath}" : "";
        ProblemText.Text = problem ?? "";
        ProblemText.Visibility = problem is null ? Visibility.Collapsed : Visibility.Visible;
        OkButton.IsEnabled = problem is null;
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        // Re-checked here as well as on every keystroke, because the folder may have gained a file
        // of that name while this sat open — the same reason NewItemDialog re-plans in its Ok.
        Update();
        if (!OkButton.IsEnabled) return;
        DialogResult = true;
    }
}
