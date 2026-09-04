using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services.Duplicates;
using Microsoft.Extensions.DependencyInjection;
using System.Windows;

namespace BertBrowser.App.Views;

/// <summary>Opens the checksum dialog for a single file, hashing it with the same
/// <see cref="IFileHasher"/> the duplicate finder uses.</summary>
internal static class ChecksumPrompt
{
    public static void Show(string fullPath)
    {
        var vm = new ChecksumViewModel(fullPath, App.Services.GetRequiredService<IFileHasher>());
        new ChecksumDialog(vm) { Owner = Application.Current?.MainWindow }.ShowDialog();
    }
}
