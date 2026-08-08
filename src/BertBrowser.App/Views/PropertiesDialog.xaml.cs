using System.Windows;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

public partial class PropertiesDialog : ThemedWindow
{
    public PropertiesDialog(PropertiesViewModel vm)
    {
        InitializeComponent();
        DataContext = vm;
        Loaded += async (_, _) => await vm.LoadAsync();
        Closed += (_, _) => vm.CancelPendingWork();
    }
}
