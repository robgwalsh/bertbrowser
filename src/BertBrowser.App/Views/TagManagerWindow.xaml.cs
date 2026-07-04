using System.Windows;
using System.Windows.Input;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

public partial class TagManagerWindow : Window
{
    private readonly TagManagerViewModel _vm;

    public TagManagerWindow(TagManagerViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;
        vm.ConfirmAction = message =>
            MessageBox.Show(this, message, "Delete tag", MessageBoxButton.YesNo, MessageBoxImage.Warning)
                == MessageBoxResult.Yes;
        Loaded += async (_, _) => await vm.LoadAsync();
    }

    private void NewTagBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter && _vm.AddTagCommand.CanExecute(null))
            _vm.AddTagCommand.Execute(null);
    }

    private void Rename_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedTag is not { } tag) return;

        var input = new InputDialog("Rename tag", $"New name for \"{tag.Name}\":", tag.Name) { Owner = this };
        if (input.ShowDialog() == true && input.Value.Length > 0)
            _vm.RenameTagCommand.Execute((tag, input.Value));
    }

    private void SetColor_Click(object sender, RoutedEventArgs e)
    {
        if (_vm.SelectedTag is not { } tag) return;

        var input = new InputDialog("Set tag color", $"Color for \"{tag.Name}\" (#RRGGBB):", tag.Color ?? "#607D8B") { Owner = this };
        if (input.ShowDialog() == true && input.Value.Length > 0)
            _vm.SetColorCommand.Execute((tag, input.Value));
    }
}
