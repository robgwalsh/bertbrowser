using System.Collections.ObjectModel;
using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Interop;
using BertBrowser.Core.Services.Columns;
using CommunityToolkit.Mvvm.ComponentModel;

namespace BertBrowser.App.Views;

/// <summary>One row of the picker: a property, and whether it is to be a column.</summary>
public sealed partial class ColumnChoice : ObservableObject
{
    public required string Canonical { get; init; }

    public required string Display { get; init; }

    [ObservableProperty]
    private bool _chosen;
}

/// <summary>
/// "More columns…": everything the Windows property system knows about on this machine, so the
/// curated list on the header menu is a shortcut rather than a ceiling.
/// </summary>
/// <remarks>
/// <para>
/// The list is <b>enumerated, not hard-coded</b>, and its labels are the localised ones the shell
/// hands back — the same reason <c>PreviewMetadata</c> matches on canonical names and displays
/// something else. The canonical name is shown under each label as well, because that is what ends
/// up in <c>settings.json</c> and it is the only way to tell two similarly-named properties apart.
/// </para>
/// <para>
/// Properties a built-in column already covers are left out (<see cref="ColumnCatalog.ShadowedByBuiltIn"/>):
/// a second Type column that is blank until it hydrates is worse than the free one that never is.
/// </para>
/// </remarks>
public partial class ColumnPickerDialog : ThemedWindow
{
    private readonly ObservableCollection<ColumnChoice> _all = [];
    private readonly HashSet<string> _initial;

    private ColumnPickerDialog(IReadOnlyList<ColumnSetting> current)
    {
        InitializeComponent();
        _initial = current.Select(c => c.Id).ToHashSet(StringComparer.OrdinalIgnoreCase);

        StatusText.Text = "Reading the property system…";
        Loaded += async (_, _) => await LoadAsync();
    }

    /// <summary>The dialog built but not shown, for the UI harness to photograph.</summary>
    internal static ColumnPickerDialog Create(Window? owner, IReadOnlyList<ColumnSetting> current) =>
        new(current) { Owner = owner };

    /// <summary>The canonical names ticked when OK was pressed.</summary>
    public IReadOnlyList<string> Chosen { get; private set; } = [];

    private async Task LoadAsync()
    {
        // Off the UI thread: this is a few hundred COM activations, and it is the reason the list
        // starts empty with a line saying so rather than freezing the window on open.
        var found = await Task.Run(() => ShellProperties.EnumerateDescriptions()
            .Where(p => !ColumnCatalog.ShadowedByBuiltIn.Contains(p.Canonical))
            .Where(p => ColumnId.LooksCanonical(p.Canonical))
            .GroupBy(p => p.Canonical, StringComparer.OrdinalIgnoreCase)
            .Select(g => g.First())
            .OrderBy(p => p.Display, StringComparer.CurrentCultureIgnoreCase)
            .ToList());

        foreach (var (canonical, display) in found)
        {
            _all.Add(new ColumnChoice
            {
                Canonical = canonical,
                Display = display,
                Chosen = _initial.Contains(canonical),
            });
        }

        StatusText.Text = found.Count == 0
            ? "This PC reported no properties."
            : $"{found.Count:N0} properties.";
        ApplyFilter();
    }

    private void Filter_Changed(object sender, TextChangedEventArgs e) => ApplyFilter();

    private void ApplyFilter()
    {
        var needle = FilterBox.Text.Trim();
        PropertyList.ItemsSource = needle.Length == 0
            ? _all
            : _all.Where(c =>
                c.Display.Contains(needle, StringComparison.CurrentCultureIgnoreCase) ||
                c.Canonical.Contains(needle, StringComparison.OrdinalIgnoreCase)).ToList();
    }

    private void Ok_Click(object sender, RoutedEventArgs e)
    {
        // Only what changed: a column already showing is left where it is rather than being removed
        // and re-added at the end, which would silently reorder the list every time this is opened.
        Chosen = _all.Where(c => c.Chosen).Select(c => c.Canonical).ToList();
        DialogResult = true;
    }
}
