using System.Windows;
using BertBrowser.App.ViewModels;

namespace BertBrowser.App.Views;

/// <summary>Live view of every directory size scan in flight. Bound to the
/// <see cref="ShellViewModel"/> so the list updates as scans report progress, finish, or
/// are cancelled; closes on its own once (and if) the user dismisses it.</summary>
public partial class ScanProgressWindow : Window
{
    public ScanProgressWindow(ShellViewModel shell)
    {
        InitializeComponent();
        DataContext = shell;
    }
}
