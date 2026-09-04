using System.Windows;

namespace BertBrowser.App.Views;

/// <summary>The save button inside this tab's search box.</summary>
public partial class DirectoryTabView
{
    private void SaveSearch_Click(object sender, RoutedEventArgs e)
    {
        var saved = _shell.SavedSearches;
        var seed = saved.SeedFor(Tab);
        if (SavedSearchDialog.Show(Window.GetWindow(this), seed, n => saved.IsNameTaken(n)) is { } result)
            _ = _shell.SaveSearchAsync(result, previousName: null);
    }
}
