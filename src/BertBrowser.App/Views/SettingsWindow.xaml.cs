using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Columns;
using Microsoft.Win32;

namespace BertBrowser.App.Views;

public partial class SettingsWindow : ThemedWindow
{
    private readonly SettingsViewModel _vm;

    public SettingsWindow(SettingsViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;

        // The reorder that replaced the up and down buttons. The drop reports two indexes; what
        // they mean is ColumnLayoutRules' business, not this window's.
        ListReorderDrag.Attach(ColumnDefaultsList, Orientation.Vertical, _vm.MoveColumn);
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.TrySave(out var error))
        {
            DialogResult = true;
        }
        else
        {
            MessageDialog.Show(this, error ?? "", "Settings", MessageDialogKind.Warning);
        }
    }

    /// <summary>
    /// The editor is modeless so its changes can be judged against the file list, which means this
    /// dialog has to get out of the way: it commits what is pending and closes.
    /// </summary>
    private void CustomiseTheme_Click(object sender, RoutedEventArgs e)
    {
        // TrySave puts the offending command on screen itself, which is what makes leaving the
        // Appearance page acceptable here.
        if (!_vm.TrySave(out var error))
        {
            MessageDialog.Show(this, error ?? "", "Settings", MessageDialogKind.Warning);
            return;
        }

        var editor = new ThemeEditorWindow(_vm.Appearance) { Owner = Application.Current?.MainWindow };
        DialogResult = true;
        editor.Show();
    }

    /// <summary>
    /// The whole property system, for the columns the curated list does not name.
    /// </summary>
    /// <remarks>
    /// A popup hung off the button rather than a modal dialog, and the only way in: there is no
    /// second "More columns…" button because the popup's own search box is what reaches past the
    /// curated list. Each click writes straight into the pending layout, which is what lets three
    /// columns be added in three clicks with nothing to confirm.
    /// </remarks>
    private void AddColumn_Click(object sender, RoutedEventArgs e) =>
        ColumnAddPopup.Show(AddColumnButton, _vm.CurrentColumns, _vm.AddColumn);

    /// <summary>
    /// The keyboard's half of the drag, plus Delete.
    /// </summary>
    /// <remarks>
    /// Alt is what makes Up and Down mean "move this" rather than "select the next one" — the same
    /// modifier every list that can be reordered by hand uses, and the reason plain arrows still
    /// walk the list.
    /// </remarks>
    private void ColumnList_KeyDown(object sender, KeyEventArgs e)
    {
        if (Keyboard.Modifiers == ModifierKeys.Alt && e.Key is Key.Up or Key.Down or Key.System)
        {
            // Alt turns the key into Key.System and puts the real one on SystemKey.
            var key = e.Key == Key.System ? e.SystemKey : e.Key;
            if (key is not (Key.Up or Key.Down)) return;

            _vm.NudgeSelectedColumn(key == Key.Up ? -1 : 1);
            RefocusSelectedColumn();
            e.Handled = true;
        }
        else if (e.Key == Key.Delete && _vm.SelectedColumn is { Removable: true } selected)
        {
            _vm.RemoveColumnCommand.Execute(selected);
            RefocusSelectedColumn();
            e.Handled = true;
        }
    }

    /// <summary>
    /// Puts the keyboard back on the selected row.
    /// </summary>
    /// <remarks>
    /// Every edit rebuilds the list from what the layout rules answered, which throws away the
    /// containers — and with them the focus this handler is reached through. Without this, Alt+Up
    /// would move a column once and then go silent.
    /// </remarks>
    private void RefocusSelectedColumn()
    {
        ColumnDefaultsList.UpdateLayout();
        if (_vm.SelectedColumn is not { } selected) return;
        if (ColumnDefaultsList.ItemContainerGenerator.ContainerFromItem(selected) is ListBoxItem row)
            row.Focus();
    }

    /// <summary>
    /// The wheel over a width box changes the width instead of scrolling the list past it.
    /// </summary>
    /// <remarks>
    /// Handled unconditionally, including at the ends of the range: letting the scroll through once
    /// the width has hit its limit would make the list lurch under a pointer that had been sitting
    /// still, which reads as a fault rather than as a limit.
    /// </remarks>
    private void ColumnWidth_Wheel(object sender, MouseWheelEventArgs e)
    {
        e.Handled = true;
        if (sender is not FrameworkElement { DataContext: ColumnItemViewModel column }) return;

        var notches = e.Delta / Mouse.MouseWheelDeltaForOneLine;
        column.Width = ColumnLayoutRules.StepWidth(
            column.Width, notches, fine: Keyboard.Modifiers.HasFlag(ModifierKeys.Control));
    }

    private void Browse_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedCommand is not { } command) return;

        var dialog = new OpenFileDialog
        {
            Title = "Choose a program",
            Filter = "Programs (*.exe;*.bat;*.cmd)|*.exe;*.bat;*.cmd|All files (*.*)|*.*",
        };
        if (dialog.ShowDialog(this) == true)
            command.Command = dialog.FileName;
    }

    private void BrowseTemplate_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedNewFileType is not { } type) return;

        var dialog = new OpenFileDialog
        {
            Title = "Choose a template file",
            Filter = "All files (*.*)|*.*",
        };
        if (dialog.ShowDialog(this) == true)
            type.TemplatePath = dialog.FileName;
    }

    private void BrowseStartupPath_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFolderDialog
        {
            Title = "Choose a startup folder",
            InitialDirectory = _vm.StartupDefaultPath,
        };
        if (dialog.ShowDialog(this) == true)
            _vm.StartupDefaultPath = dialog.FolderName;
    }
}
