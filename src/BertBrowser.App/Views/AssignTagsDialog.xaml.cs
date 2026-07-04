using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

public partial class AssignTagsDialog : Window
{
    private readonly AssignTagsViewModel _vm;

    public AssignTagsDialog(AssignTagsViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;
        Loaded += async (_, _) => await vm.LoadAsync();
    }

    private void NewTagBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && _vm.CreateTagCommand.CanExecute(null))
            _vm.CreateTagCommand.Execute(null);
    }

    private async void Ok_Click(object sender, RoutedEventArgs e)
    {
        await _vm.ApplyAsync();
        DialogResult = true;
    }
}
