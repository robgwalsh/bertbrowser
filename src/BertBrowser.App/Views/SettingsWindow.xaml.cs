using System.Windows;
using BertBrowser.App.ViewModels;
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
    /// Modal over Settings rather than closing it the way "Customise colours…" does: this one
    /// answers with a list and comes straight back, so there is nothing to get out of the way of.
    /// The result goes through <c>ColumnLayoutRules.ApplyPicked</c>, the same rule the header menu's
    /// picker uses.
    /// </remarks>
    private void MoreColumns_Click(object sender, RoutedEventArgs e)
    {
        var dialog = ColumnPickerDialog.Create(this, _vm.ColumnsForPicker());
        if (dialog.ShowDialog() == true) _vm.ApplyPickedColumns(dialog.Chosen);
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
}
