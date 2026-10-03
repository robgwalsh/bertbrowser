using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>Picks a row's shape by what kind of row it is.</summary>
public sealed class PaletteRowTemplateSelector : DataTemplateSelector
{
    public DataTemplate? Heading { get; set; }

    public DataTemplate? Entry { get; set; }

    public DataTemplate? Category { get; set; }

    public override DataTemplate? SelectTemplate(object item, DependencyObject container) =>
        item is PaletteRowViewModel row
            ? row.IsHeading ? Heading : row.IsCategory ? Category : Entry
            : base.SelectTemplate(item, container);
}

/// <summary>
/// The palette's view. The box keeps the keyboard for as long as the palette is up: the arrows,
/// Enter, Tab and Esc are handled on the way <em>into</em> it, so the list is steered without ever
/// taking focus and typing is never interrupted.
/// </summary>
public partial class CommandPalette : UserControl
{
    /// <summary>How far PageUp and PageDown move.</summary>
    private const int Page = 8;

    private CommandPaletteViewModel? _vm;

    public CommandPalette()
    {
        InitializeComponent();
        DataContextChanged += (_, _) => Attach(DataContext as CommandPaletteViewModel);
    }

    private void Attach(CommandPaletteViewModel? vm)
    {
        if (_vm is not null)
        {
            _vm.PropertyChanged -= OnViewModelChanged;
            _vm.CaretToEndRequested -= CaretToEnd;
        }

        _vm = vm;
        if (vm is null) return;

        vm.PropertyChanged += OnViewModelChanged;
        vm.CaretToEndRequested += CaretToEnd;
        UpdatePlaceholder();
    }

    /// <summary>Puts the caret in the box, at the end of whatever it was opened with.</summary>
    internal void FocusBox()
    {
        QueryBox.Focus();
        CaretToEnd();
    }

    private void CaretToEnd() => QueryBox.CaretIndex = QueryBox.Text.Length;

    private void OnViewModelChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(CommandPaletteViewModel.Query)) UpdatePlaceholder();
    }

    private void UpdatePlaceholder() =>
        Placeholder.Visibility = string.IsNullOrEmpty(_vm?.Query) ? Visibility.Visible : Visibility.Collapsed;

    private void QueryBox_PreviewKeyDown(object sender, KeyEventArgs e)
    {
        if (_vm is null) return;

        // An Alt chord arrives as Key.System with the real key in SystemKey.
        var key = e.Key == Key.System ? e.SystemKey : e.Key;
        var alt = Keyboard.Modifiers == ModifierKeys.Alt;
        var plain = Keyboard.Modifiers == ModifierKeys.None;

        switch (key)
        {
            case Key.Down when plain: _vm.Move(1); break;
            case Key.Up when plain: _vm.Move(-1); break;
            case Key.PageDown when plain: Step(Page); break;
            case Key.PageUp when plain: Step(-Page); break;
            case Key.Enter when plain: _vm.Activate(); break;
            // Tab is kept here rather than moving focus: the palette has one field, and Tab is
            // how a path or a category is carried on from.
            case Key.Tab when plain: _vm.Complete(); break;
            case Key.Escape when plain: _vm.Dismiss(); break;
            case Key.P when alt: _vm.TogglePin(); break;
            case Key.K when alt: _vm.ChangeShortcut(); break;
            default: return;
        }

        e.Handled = true;
    }

    /// <summary>A page at a time, stopping at the ends rather than wrapping past them.</summary>
    private void Step(int rows)
    {
        if (_vm is null) return;

        var choosable = _vm.Rows.Where(r => !r.IsHeading).ToList();
        var at = _vm.SelectedRow is null ? -1 : choosable.IndexOf(_vm.SelectedRow);
        var target = at + rows;

        if (target <= 0) _vm.MoveToEdge(last: false);
        else if (target >= choosable.Count - 1) _vm.MoveToEdge(last: true);
        else _vm.SelectedRow = choosable[target];
    }

    private void RowList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (RowList.SelectedItem is { } row) RowList.ScrollIntoView(row);
    }

    /// <summary>A click on a row chooses it — but not a click on one of the row's own buttons,
    /// which has already done what it was for.</summary>
    private void RowList_PreviewMouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (_vm is null) return;
        if (VisualTreeUtil.FindAncestor<Button>(e.OriginalSource as DependencyObject) is not null) return;
        if (VisualTreeUtil.FindAncestor<ListBoxItem>(e.OriginalSource as DependencyObject)
            is not { DataContext: PaletteRowViewModel { IsHeading: false } row }) return;

        _vm.SelectedRow = row;
        _vm.Activate();
        e.Handled = true;
    }

    private void Pin_Click(object sender, RoutedEventArgs e)
    {
        _vm?.TogglePin((sender as FrameworkElement)?.DataContext as PaletteRowViewModel);
        QueryBox.Focus();
    }

    private void Bind_Click(object sender, RoutedEventArgs e) =>
        _vm?.ChangeShortcut((sender as FrameworkElement)?.DataContext as PaletteRowViewModel);

    /// <summary>A press outside the panel means "never mind".</summary>
    private void Backdrop_MouseDown(object sender, MouseButtonEventArgs e) => _vm?.Dismiss();

    /// <summary>A press inside it must not reach the backdrop, and must not take the keyboard
    /// from the box.</summary>
    private void Panel_MouseDown(object sender, MouseButtonEventArgs e)
    {
        e.Handled = true;
        QueryBox.Focus();
    }
}
