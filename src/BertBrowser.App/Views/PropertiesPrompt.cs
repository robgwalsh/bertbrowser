using BertBrowser.App.ViewModels;
using BertBrowser.Core.Data;
using Microsoft.Extensions.DependencyInjection;
using System.Windows;

namespace BertBrowser.App.Views;

/// <summary>Opens the properties dialog for a whole selection; with more than one item it shows
/// aggregates and edits the shared attributes in bulk.</summary>
internal static class PropertiesPrompt
{
    /// <summary>Returns true when attributes were changed, so the caller can reload — toggling the
    /// hidden bit can add or remove rows.</summary>
    public static bool Show(IReadOnlyList<PropertiesTarget> targets)
    {
        if (targets.Count == 0) return false;

        var vm = new PropertiesViewModel(targets,
            App.Services.GetRequiredService<DirSizeRepository>());
        new PropertiesDialog(vm) { Owner = Application.Current?.MainWindow }.ShowDialog();
        return vm.AttributesChanged;
    }

    public static bool Show(IEnumerable<FileItemViewModel> items) =>
        Show(items.Select(i => new PropertiesTarget(i.FullPath, i.IsDirectory)).ToList());

    public static bool Show(string fullPath, bool isDirectory) =>
        Show([new PropertiesTarget(fullPath, isDirectory)]);
}
