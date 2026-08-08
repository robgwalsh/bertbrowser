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
        if (!_vm.TrySave(out var error))
        {
            MessageDialog.Show(this, error ?? "", "Settings", MessageDialogKind.Warning);
            return;
        }

        var editor = new ThemeEditorWindow(_vm.Appearance) { Owner = Application.Current?.MainWindow };
        DialogResult = true;
        editor.Show();
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
}
