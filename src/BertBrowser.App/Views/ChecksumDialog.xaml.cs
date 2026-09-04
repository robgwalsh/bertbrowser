using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

public partial class ChecksumDialog : ThemedWindow
{
    public ChecksumDialog(ChecksumViewModel vm)
    {
        InitializeComponent();
        DataContext = vm;
        Loaded += async (_, _) => await vm.HashAsync();
        Closed += (_, _) => vm.Dispose();
    }
}
