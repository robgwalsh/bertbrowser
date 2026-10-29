using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using BertBrowser.Core.Services.CommandBars;
using IconPath = System.Windows.Shapes.Path;

namespace BertBrowser.App.Views;

/// <summary>What a toolbar needs done for it by the window that owns it.</summary>
/// <param name="Run">Runs a command by id, through the window's one gate.</param>
/// <param name="ListRows">The menu rows a list expands to right now.</param>
/// <param name="MenuElements">A submenu's rows as menu items.</param>
internal sealed record CommandBarToolbarHost(
    Action<string> Run,
    Func<string, List<FrameworkElement>> ListRows,
    Func<IReadOnlyList<BarRow>, List<FrameworkElement>> MenuElements);

/// <summary>
/// One toolbar drawn into a panel: the title bar's, or a row under it. It is handed the rows
/// <see cref="CommandBarRules.Present"/> worked out and keeps the buttons in step with them.
/// </summary>
/// <remarks>
/// <para>
/// A refresh <em>updates the buttons it has</em> whenever the rows are the same commands in the
/// same order, and only rebuilds when the arrangement itself changed. That is not an
/// optimisation: a refresh is asked for on almost every input event, a press on a button is one,
/// and a button replaced between its mouse-down and its mouse-up never raises Click.
/// </para>
/// <para>
/// No button here is focusable. A toolbar click must leave the keyboard where it was — in the
/// file list — or the command it runs acts on a list that no longer has focus, and the next key
/// goes to the button.
/// </para>
/// </remarks>
internal sealed class CommandBarToolbar
{
    /// <summary>Room kept for the overflow button when not everything fits.</summary>
    private const double OverflowWidth = 30;

    private readonly Panel _panel;
    private readonly CommandBarToolbarHost _host;
    private readonly Func<int, List<FrameworkElement>> _overflowRows;
    private readonly Button _overflow;
    private readonly List<FrameworkElement> _elements = [];
    private double[] _widths = [];
    private bool _widthsStale;

    /// <param name="overflowRows">The rows from this index on, as menu items, for the "more"
    /// button's dropdown.</param>
    public CommandBarToolbar(Panel panel, CommandBarToolbarHost host, Func<int, List<FrameworkElement>> overflowRows)
    {
        _panel = panel;
        _host = host;
        _overflowRows = overflowRows;

        _overflow = Styled(new Button { ToolTip = "More", Visibility = Visibility.Collapsed, Content = Icon("Icon.ChevronDown", 12) });
        AutomationProperties.SetName(_overflow, "More toolbar buttons");
        _overflow.Click += (_, _) => OpenBelow(_overflow, _overflowRows(VisibleCount));
    }

    /// <summary>What is on the toolbar, shown or overflowed. For the UI harness.</summary>
    internal IReadOnlyList<BarRow> Rows { get; private set; } = [];

    /// <summary>How many of <see cref="Rows"/> are showing; the rest are behind the overflow button.</summary>
    internal int VisibleCount { get; private set; }

    public void Show(IReadOnlyList<BarRow> rows)
    {
        if (SameShape(rows, Rows))
        {
            for (var i = 0; i < rows.Count; i++)
            {
                // A caption shown beside the icon changes the button's width when it changes.
                if (rows[i].ShowText && rows[i].Caption != Rows[i].Caption) _widthsStale = true;
                Update(_elements[i], rows[i]);
            }
            Rows = rows;
            return;
        }

        Rows = rows;
        _elements.Clear();
        _panel.Children.Clear();
        foreach (var row in rows)
        {
            var element = Build(row);
            Update(element, row);
            _elements.Add(element);
            _panel.Children.Add(element);
        }
        _panel.Children.Add(_overflow);
        _widthsStale = true;
        VisibleCount = rows.Count;
    }

    /// <summary>
    /// Shows as many items as fit in <paramref name="available"/> and puts the rest behind the
    /// overflow button. The widths are each item's own natural width, measured with nothing
    /// hidden, so the answer never depends on what the last answer hid.
    /// </summary>
    public void Fit(double available)
    {
        if (_widthsStale)
        {
            _widths = new double[_elements.Count];
            for (var i = 0; i < _elements.Count; i++)
            {
                _elements[i].Visibility = Visibility.Visible;
                _elements[i].Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
                _widths[i] = _elements[i].DesiredSize.Width;
            }
            _widthsStale = false;
        }

        var count = double.IsFinite(available)
            ? CommandBarRules.Fit(_widths, available, OverflowWidth)
            : _elements.Count;
        // A rule with nothing after it is a rule at the edge.
        while (count > 0 && count < Rows.Count && Rows[count - 1].Kind == BarRowKind.Separator) count--;

        for (var i = 0; i < _elements.Count; i++)
            _elements[i].Visibility = i < count ? Visibility.Visible : Visibility.Collapsed;
        _overflow.Visibility = count < _elements.Count ? Visibility.Visible : Visibility.Collapsed;
        VisibleCount = count;
    }

    private static bool SameShape(IReadOnlyList<BarRow> a, IReadOnlyList<BarRow> b) =>
        a.Count == b.Count && a.Zip(b).All(p =>
            p.First.Kind == p.Second.Kind && p.First.Id == p.Second.Id && p.First.Icon == p.Second.Icon &&
            p.First.ShowText == p.Second.ShowText &&
            (p.First.Kind != BarRowKind.Menu || p.First.Caption == p.Second.Caption));

    // --- Building ---

    private FrameworkElement Build(BarRow row)
    {
        switch (row.Kind)
        {
            case BarRowKind.Separator:
            {
                var rule = new Border { Width = 1, Height = 18, Margin = new Thickness(6, 0, 6, 0), VerticalAlignment = VerticalAlignment.Center };
                // The bar's own text colour, faint: a border token is tuned against a page, and all
                // but vanishes on the title bar.
                rule.SetResourceReference(Border.BackgroundProperty, "Theme.TitleBar.Foreground");
                rule.Opacity = 0.28;
                return rule;
            }

            case BarRowKind.Command when CommandBarRules.DropdownFor(row.Id!) is { } list:
            {
                // A split button: the face runs the command, the chevron lists what it would undo.
                var face = Styled(new Button { Margin = new Thickness(2, 0, 0, 0), Padding = new Thickness(8, 4, 4, 4) });
                face.Click += (_, _) => RunFace(face);

                var chevron = Styled(new Button
                {
                    Margin = new Thickness(0, 0, 2, 0), Padding = new Thickness(2, 4, 2, 4), MinWidth = 14,
                    Content = Icon("Icon.ChevronDown", 10),
                });
                chevron.Click += (_, _) => OpenBelow(chevron, _host.ListRows(list));

                var split = new StackPanel { Orientation = Orientation.Horizontal };
                split.Children.Add(face);
                split.Children.Add(chevron);
                return split;
            }

            case BarRowKind.Command:
            {
                var button = Styled(new Button());
                button.Click += (_, _) => RunFace(button);
                return button;
            }

            // A submenu or a list on a toolbar is a button that drops it down.
            default:
            {
                var button = Styled(new Button());
                button.Click += (_, _) =>
                {
                    if (button.Tag is not BarRow now) return;
                    OpenBelow(button, now.Kind == BarRowKind.Menu ? _host.MenuElements(now.Children) : _host.ListRows(now.Id!));
                };
                return button;
            }
        }
    }

    private void RunFace(Button face)
    {
        if (face.Tag is BarRow { Id: { } id }) _host.Run(id);
    }

    private static void Update(FrameworkElement element, BarRow row)
    {
        switch (element)
        {
            case StackPanel { Children: [Button face, Button chevron] }:
                UpdateButton(face, row, dropdown: false);
                chevron.ToolTip = $"{row.Name} history";
                AutomationProperties.SetName(chevron, $"{row.Name} history");
                break;
            case Button button:
                UpdateButton(button, row, dropdown: row.Kind != BarRowKind.Command);
                break;
        }
    }

    private static void UpdateButton(Button button, BarRow row, bool dropdown)
    {
        // Nearly every refresh finds nearly every button as it was.
        if (button.Tag is BarRow shown && shown == row) return;

        button.Tag = row;
        button.IsEnabled = row.IsEnabled;
        AutomationProperties.SetName(button, row.Name);

        // What it is and its shortcut — or, greyed, why not: the palette's rule, on a button.
        button.ToolTip = !row.IsEnabled && row.Reason is { } reason ? $"{row.Name} — {reason}"
            : row.Gesture.Length > 0 ? $"{row.Caption} ({row.Gesture})"
            : row.Caption;

        if (row.IsChecked) button.SetResourceReference(Control.BackgroundProperty, "Theme.Toolbar.PressedBackground");
        else button.ClearValue(Control.BackgroundProperty);

        // A menu has no icon, so its name is all it has; so has a command that names no picture.
        var text = row.ShowText || row.Icon is null || row.Kind == BarRowKind.Menu;
        var content = new StackPanel { Orientation = Orientation.Horizontal };
        if (row.Icon is { } icon && row.Kind != BarRowKind.Menu) content.Children.Add(Icon(icon, 16));
        if (text)
        {
            content.Children.Add(new TextBlock
            {
                Text = row.Caption,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = new Thickness(content.Children.Count > 0 ? 6 : 0, 0, 0, 0),
            });
        }
        if (dropdown)
        {
            var chevron = Icon("Icon.ChevronDown", 10);
            chevron.Margin = new Thickness(5, 1, 0, 0);
            content.Children.Add(chevron);
        }
        button.Content = content;
    }

    private static Button Styled(Button button)
    {
        button.Focusable = false;
        button.SetResourceReference(FrameworkElement.StyleProperty, "ToolbarButton");
        ToolTipService.SetShowOnDisabled(button, true);
        return button;
    }

    /// <summary>A resource-referenced outline, so it follows a theme change like any declared in XAML.</summary>
    private static IconPath Icon(string key, double size)
    {
        var icon = new IconPath { Width = size, Height = size };
        icon.SetResourceReference(FrameworkElement.StyleProperty, "IconPath");
        icon.SetResourceReference(IconPath.DataProperty, key);
        return icon;
    }

    private static void OpenBelow(UIElement anchor, List<FrameworkElement> items)
    {
        if (items.Count == 0) return;

        var menu = new ContextMenu { PlacementTarget = anchor, Placement = PlacementMode.Bottom, VerticalOffset = 2 };
        foreach (var item in items) menu.Items.Add(item);
        menu.IsOpen = true;
    }
}
