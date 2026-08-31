using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using BertBrowser.Core.Services.Columns;

namespace BertBrowser.App.Views;

/// <summary>Which sort marker a column header carries, if any.</summary>
public enum ColumnSortGlyph
{
    None,
    Ascending,
    Descending,
}

/// <summary>
/// Builds the file list's <see cref="GridViewColumn"/>s from resolved column specs, and remembers on
/// each one which spec it came from.
/// </summary>
/// <remarks>
/// <para>
/// The id is an attached property on the column rather than something read back off its header.
/// Reaching through <c>column.Header as GridViewColumnHeader</c> happens to work while every header
/// is an element, and stops working the moment one is a plain string — a silent failure that would
/// show up as columns quietly refusing to reorder.
/// </para>
/// <para>
/// The header also carries the id in its <c>Tag</c>, but <b>only when the column is sortable</b>:
/// <c>FileList_HeaderClick</c> reads that Tag, and Match has nothing to sort by.
/// </para>
/// </remarks>
internal static class FileListColumns
{
    public static readonly DependencyProperty IdProperty = DependencyProperty.RegisterAttached(
        "Id", typeof(string), typeof(FileListColumns), new PropertyMetadata(""));

    public static string GetId(DependencyObject element) => (string)element.GetValue(IdProperty);

    public static void SetId(DependencyObject element, string value) => element.SetValue(IdProperty, value);

    /// <summary>
    /// Which way this header's column is sorted, if it is the one in force.
    /// </summary>
    /// <remarks>
    /// An attached property rather than something the header's content carries, so the label stays a
    /// plain string: <see cref="UpdateHeader"/> compares it, the menu shows it, and a glyph baked
    /// into the text would have to be parsed back out of all three.
    /// </remarks>
    public static readonly DependencyProperty SortGlyphProperty = DependencyProperty.RegisterAttached(
        "SortGlyph", typeof(ColumnSortGlyph), typeof(FileListColumns),
        new PropertyMetadata(ColumnSortGlyph.None));

    public static ColumnSortGlyph GetSortGlyph(DependencyObject element) =>
        (ColumnSortGlyph)element.GetValue(SortGlyphProperty);

    public static void SetSortGlyph(DependencyObject element, ColumnSortGlyph value) =>
        element.SetValue(SortGlyphProperty, value);

    /// <summary>Marks the header of <paramref name="sortedId"/> and clears every other one.</summary>
    public static void ApplySortGlyphs(
        IEnumerable<GridViewColumn> columns, string sortedId, bool descending)
    {
        foreach (var column in columns)
        {
            if (column.Header is not GridViewColumnHeader header) continue;

            SetSortGlyph(header, GetId(column).Equals(sortedId, StringComparison.OrdinalIgnoreCase)
                ? descending ? ColumnSortGlyph.Descending : ColumnSortGlyph.Ascending
                : ColumnSortGlyph.None);
        }
    }

    /// <summary>The cell templates in <c>Resources/Styles.xaml</c> are keyed by this.</summary>
    public static string TemplateKeyFor(string id) => "Column.Cell." + id;

    public static GridViewColumn Build(ResolvedColumn column, FrameworkElement host, ContextMenu? menu = null)
    {
        var spec = column.Spec;
        var header = new GridViewColumnHeader { Content = spec.Header, ContextMenu = menu };
        SetId(header, spec.Id);
        if (spec.Sortable) header.Tag = spec.Id;
        if (spec.RightAligned) header.HorizontalContentAlignment = HorizontalAlignment.Right;

        if (host.TryFindResource("Column.Header") is DataTemplate headerTemplate)
            header.ContentTemplate = headerTemplate;

        var built = new GridViewColumn { Header = header, Width = column.Width };
        SetId(built, spec.Id);

        if (host.TryFindResource(TemplateKeyFor(spec.Id)) is DataTemplate template)
        {
            built.CellTemplate = template;
        }
        else
        {
            // Everything without a hand-written template — every shell property — renders as text
            // through the row's metadata facade. DisplayMemberBinding rather than a generated
            // template: it is a plain settable property, and one less thing to build per column.
            built.DisplayMemberBinding = BindingFor(spec);
        }
        return built;
    }

    /// <summary>Refreshes what an existing column shows without replacing it, so a reorder or a
    /// rename of a shell column's header does not cost the row containers a rebuild.</summary>
    public static void UpdateHeader(GridViewColumn column, ResolvedColumn resolved)
    {
        if (column.Header is not GridViewColumnHeader header) return;
        if (!Equals(header.Content, resolved.Spec.Header)) header.Content = resolved.Spec.Header;
    }

    /// <summary>
    /// The cell binding for a shell column: the row's metadata indexer, keyed by canonical name.
    /// </summary>
    /// <remarks>
    /// The refresh comes from <c>FileItemViewModel.NotifyColumnsChanged</c>, which raises the
    /// <c>Columns</c> property rather than <c>"Item[]"</c> — see the remarks there. Getting that
    /// wrong renders every such column permanently blank while every value is read and cached, and
    /// nothing reading the view model in C# can see the difference.
    /// </remarks>
    private static Binding? BindingFor(ColumnSpec spec) =>
        spec.Kind == ColumnKind.ShellProperty
            ? new Binding($"Columns[{spec.Id}]") { Mode = BindingMode.OneWay }
            : null;
}
