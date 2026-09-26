using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using Microsoft.Extensions.DependencyInjection;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services;
using BertBrowser.Core.Services.UndoHistory;

namespace BertBrowser.App.Views;

/// <summary>
/// The undo history's surfaces: the title bar's Undo and Redo split buttons, their dropdowns, and the
/// History window. Every action is a line into the shell, which owns the history and the busy flag.
/// </summary>
public partial class MainWindow
{
    /// <summary>How many entries a dropdown lists before "Show history…" is the way to the rest.</summary>
    internal const int MenuDepth = 12;

    private UndoHistoryWindow? _undoHistory;

    private void UndoMenu_Click(object sender, RoutedEventArgs e) =>
        OpenUndoMenu(UndoMenuButton, BuildUndoMenuItems());

    private void RedoMenu_Click(object sender, RoutedEventArgs e) =>
        OpenUndoMenu(RedoMenuButton, BuildRedoMenuItems());

    private static void OpenUndoMenu(UIElement anchor, List<FrameworkElement> items)
    {
        var menu = new ContextMenu { PlacementTarget = anchor, Placement = PlacementMode.Bottom, VerticalOffset = 2 };
        foreach (var item in items) menu.Items.Add(item);
        menu.IsOpen = true;
    }

    private void UndoHistoryLink_Click(object sender, RoutedEventArgs e) => ShowUndoHistory();

    /// <summary>The Undo dropdown: what is in effect, most recent first.</summary>
    /// <param name="poseHover">Highlights the range as though the pointer were over this item, so
    /// the harness can photograph what hovering does.</param>
    internal List<FrameworkElement> BuildUndoMenuItems(int? poseHover = null) =>
        BuildStepMenu(_shell.History.Undoable, UndoDirection.Undo, poseHover);

    /// <summary>The Redo dropdown: what has been undone, next redo first.</summary>
    internal List<FrameworkElement> BuildRedoMenuItems(int? poseHover = null) =>
        BuildStepMenu(_shell.History.Redoable, UndoDirection.Redo, poseHover);

    /// <summary>
    /// A dropdown in the shape every editor's undo list has: pointing at an entry marks it and
    /// everything above it, because that is what clicking it will take back, and the line at the
    /// bottom says how many that is.
    /// </summary>
    private List<FrameworkElement> BuildStepMenu(
        IReadOnlyList<UndoEntryViewModel> rows, UndoDirection direction, int? poseHover)
    {
        var verb = direction == UndoDirection.Undo ? "Undo" : "Redo";
        var items = new List<FrameworkElement>();
        var steps = new List<MenuItem>();
        var now = DateTime.UtcNow;

        var summary = new MenuItem { IsEnabled = false };

        void Mark(int through)
        {
            for (var i = 0; i < steps.Count; i++)
            {
                if (i <= through)
                {
                    steps[i].SetResourceReference(BackgroundProperty, "Theme.Menu.HoverBackground");
                    steps[i].SetResourceReference(ForegroundProperty, "Theme.Menu.HoverForeground");
                }
                else
                {
                    steps[i].ClearValue(BackgroundProperty);
                    steps[i].ClearValue(ForegroundProperty);
                }
            }
            summary.Header = through < 0
                ? $"Point at an action to {verb.ToLowerInvariant()} {(direction == UndoDirection.Undo ? "back" : "up")} to it"
                : $"{verb} {UndoText.Items(through + 1).Replace("item", "action")}";
        }

        foreach (var row in rows.Take(MenuDepth))
        {
            var index = steps.Count;
            var item = new MenuItem
            {
                // "__" so underscores in names render instead of becoming access keys.
                Header = row.Description.Replace("_", "__"),
                InputGestureText = RelativeTime.Format(row.Entry.ChangedUtc, now),
                Icon = MenuIcon(row.IconKey),
                ToolTip = row.HasProblem ? row.LastProblem : null,
            };
            item.MouseEnter += (_, _) => Mark(index);
            item.Click += (_, _) => _ = direction == UndoDirection.Undo
                ? _shell.UndoToAsync(row.Entry)
                : _shell.RedoToAsync(row.Entry);
            steps.Add(item);
            items.Add(item);
        }

        if (steps.Count == 0)
        {
            items.Add(new MenuItem { Header = $"Nothing to {verb.ToLowerInvariant()}", IsEnabled = false });
        }
        else
        {
            if (rows.Count > MenuDepth)
                items.Add(new MenuItem { Header = $"…and {rows.Count - MenuDepth:N0} older in the history", IsEnabled = false });
            items.Add(new Separator());
            items.Add(summary);
            Mark(poseHover is { } posed ? Math.Min(posed, steps.Count - 1) : -1);
        }

        items.Add(new Separator());
        var show = new MenuItem { Header = "Show history…", InputGestureText = "Ctrl+Alt+Z", Icon = MenuIcon("Icon.Undo") };
        show.Click += (_, _) => ShowUndoHistory();
        items.Add(show);

        return items;
    }

    /// <summary>Opens the History window, or brings the open one forward.</summary>
    private void ShowUndoHistory()
    {
        if (_undoHistory is { IsLoaded: true })
        {
            _undoHistory.Activate();
            return;
        }

        _undoHistory = UndoHistoryWindow.Create(
            _shell,
            App.Services.GetRequiredService<BertBrowser.Core.Services.IUserConfirm>(),
            RevealFromDiskUsage,
            () =>
            {
                Activate();
                ShowSettings(SettingsCategory.History);
            });
        _undoHistory.Owner = this;
        _undoHistory.Closed += (_, _) => _undoHistory = null;
        _undoHistory.Show();
    }
}
