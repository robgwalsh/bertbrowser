using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.Interop;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.Views;

/// <summary>A section heading in the add list. Never selectable — see the container style.</summary>
public sealed record ColumnAddHeading(string Title)
{
    public bool IsHeading => true;
}

/// <summary>A column the list offers. <see cref="IsHeading"/> is what the container style reads to
/// tell the two rows apart, which is why it is stated on both rather than type-tested in XAML.</summary>
public sealed record ColumnAddChoice(ColumnCandidate Candidate)
{
    public bool IsHeading => false;

    public string Id => Candidate.Id;

    public string Header => Candidate.Header;

    public string Detail => Candidate.Detail;
}

/// <summary>
/// The "Add column" list: the curated columns, the whole property system beneath them, and one
/// search box across both.
/// </summary>
/// <remarks>
/// <para>
/// A panel rather than a window because it is shown in a <see cref="ColumnAddPopup"/> from two
/// places — the settings page's Add button and the header menu's "More columns…" — and because a
/// modal dialog with an OK on it was the Windows-y thing this replaced. Clicking a row raises
/// <see cref="Chosen"/> and the row leaves the list; the popup stays open, so adding three columns
/// is three clicks.
/// </para>
/// <para>
/// What is on offer is decided by <see cref="ColumnCandidates"/> in Core, not here: this file is the
/// keystrokes, the clicks and the one COM call.
/// </para>
/// </remarks>
public partial class ColumnAddPanel : UserControl
{
    /// <summary>
    /// The property system, read once per run.
    /// </summary>
    /// <remarks>
    /// A few hundred COM activations. It is the reason the list appears with the curated half
    /// filled and the rest arriving a moment later, and the reason opening the popup a second time
    /// is instant. The set does not change while the app runs — a property handler is installed by
    /// an installer, not by using a file browser.
    /// </remarks>
    private static Task<IReadOnlyList<(string Canonical, string Display)>>? _properties;

    private Func<IReadOnlyList<ColumnSetting>?> _read = () => null;
    private IReadOnlyList<(string Canonical, string Display)> _machine = [];
    private bool _loaded;

    public ColumnAddPanel()
    {
        InitializeComponent();
        SearchBox.TextChanged += (_, _) =>
            SearchPlaceholder.Visibility =
                SearchBox.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        Loaded += async (_, _) => await LoadAsync();
    }

    /// <summary>Raised for each column clicked. The layout is not written here: the two hosts own
    /// their own layouts (a tab's, or the settings page's pending one) and write it their way.</summary>
    public event Action<string>? Chosen;

    /// <summary>Where the current layout is read from, so a column added a second ago is gone from
    /// the list the moment it is added rather than at the next open.</summary>
    public void Bind(Func<IReadOnlyList<ColumnSetting>?> read)
    {
        _read = read;
        Refresh();
    }

    /// <summary>Re-reads the layout and rebuilds the list. Called after every add.</summary>
    public void Refresh()
    {
        var candidates = ColumnCandidates.Build(_read(), _machine, SearchBox.Text, _loaded);

        var rows = new List<object>();
        foreach (var group in candidates.Groups)
        {
            rows.Add(new ColumnAddHeading(group.Title));
            foreach (var candidate in group.Items) rows.Add(new ColumnAddChoice(candidate));
        }
        CandidateList.ItemsSource = rows;

        StatusText.Text = Status(candidates);
        StatusText.Visibility = StatusText.Text.Length == 0 ? Visibility.Collapsed : Visibility.Visible;
    }

    /// <summary>Says which empty this is. "Nothing here" over a blank list is the one thing a person
    /// cannot tell from a broken list.</summary>
    private static string Status(ColumnCandidates candidates)
    {
        if (candidates.IsFull)
            return $"That is all {ColumnLayoutRules.MaxColumns} columns. Remove one to add another.";
        if (candidates.IsLoading)
            return "Reading the rest of the property system…";
        return candidates.IsEmpty ? "No property matches that." : "";
    }

    /// <summary>
    /// Starts the property-system read, or joins one already running.
    /// </summary>
    /// <remarks>
    /// Off the UI thread, and shared: a second panel opened while the first is still reading waits
    /// on the same task rather than enumerating the machine again. Exposed so the UI harness can
    /// wait for it before photographing the panel — the list is worth nothing as a picture while it
    /// still says it is reading.
    /// </remarks>
    internal static Task<IReadOnlyList<(string Canonical, string Display)>> Preload() =>
        _properties ??= Task.Run<IReadOnlyList<(string, string)>>(ShellProperties.EnumerateDescriptions);

    private async Task LoadAsync()
    {
        if (_loaded) return;

        _machine = await Preload();
        _loaded = true;
        Refresh();
    }

    private void Search_Changed(object sender, TextChangedEventArgs e) => Refresh();

    /// <summary>
    /// Down out of the search box moves into the list, and Enter takes the first match.
    /// </summary>
    /// <remarks>
    /// Without this the box keeps the focus and the list can only be reached with the mouse, which
    /// would make the search half of this panel typing-only — the thing that made the old dialog's
    /// filter box feel like a chore.
    /// </remarks>
    private void Search_KeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Down:
                if (FirstChoice() is { } item)
                {
                    CandidateList.SelectedItem = item;
                    Container(item)?.Focus();
                }
                e.Handled = true;
                break;

            case Key.Enter:
                if (FirstChoice() is { } first) Add(first);
                e.Handled = true;
                break;
        }
    }

    private void List_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key is Key.Enter or Key.Space && CandidateList.SelectedItem is ColumnAddChoice choice)
        {
            Add(choice);
            e.Handled = true;
        }
        else if (e.Key == Key.Up && ReferenceEquals(CandidateList.SelectedItem, FirstChoice()))
        {
            // Back out of the top of the list into the box, rather than sitting on the first row
            // with nowhere to go.
            SearchBox.Focus();
            SearchBox.CaretIndex = SearchBox.Text.Length;
            e.Handled = true;
        }
    }

    /// <summary>
    /// PreviewMouseLeftButtonUp, not a click on the item.
    /// </summary>
    /// <remarks>
    /// Down-then-up on the same row is what a person means by clicking it, and the up is where the
    /// list has already moved its selection. Handling the <em>down</em> instead would add a column
    /// on a click that started here and ended somewhere else.
    /// </remarks>
    private void Candidate_Click(object sender, MouseButtonEventArgs e)
    {
        if (e.OriginalSource is not DependencyObject source) return;
        if (VisualTreeUtil.FindAncestor<ListBoxItem>(source) is not { DataContext: ColumnAddChoice choice })
            return;

        Add(choice);
        e.Handled = true;
    }

    private void Add(ColumnAddChoice choice)
    {
        Chosen?.Invoke(choice.Id);

        // The host has written the layout by now, so this both removes the row just taken and picks
        // up anything else the host changed.
        Refresh();

        // Keep the keyboard where the person left it: a search then Enter then more typing is a
        // whole session, and dropping focus into the list after each add would break it.
        if (SearchBox.IsKeyboardFocusWithin) return;
        if (FirstChoice() is { } next)
        {
            CandidateList.SelectedItem = next;
            Container(next)?.Focus();
        }
        else
        {
            SearchBox.Focus();
        }
    }

    private ColumnAddChoice? FirstChoice() =>
        CandidateList.ItemsSource?.OfType<ColumnAddChoice>().FirstOrDefault();

    /// <summary>The row for an item. UpdateLayout first: the container for a row that arrived with
    /// the ItemsSource set a moment ago has not been generated yet, and a null here would silently
    /// mean "no keyboard focus" rather than failing.</summary>
    private ListBoxItem? Container(object item)
    {
        CandidateList.UpdateLayout();
        return CandidateList.ItemContainerGenerator.ContainerFromItem(item) as ListBoxItem;
    }
}
