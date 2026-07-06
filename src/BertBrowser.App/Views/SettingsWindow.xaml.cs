using System.Windows;
using BertBrowser.App.ViewModels;
using Microsoft.Win32;

namespace BertBrowser.App.Views;

public partial class SettingsWindow : Window
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
            MessageBox.Show(this, error, "Settings", MessageBoxButton.OK, MessageBoxImage.Warning);
        }
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
