using System.Windows;
using System.Windows.Controls;
using BertBrowser.App.Theming;
using BertBrowser.App.ViewModels;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// "What is taking up my disk?" — the biggest files under a root, and what a folder is made of,
/// drillable. Every number comes from the index the MFT pass already built; nothing here scans.
/// </summary>
/// <remarks>
/// Modeless on purpose, for the reason the theme editor is: acting on what it says means going to
/// a folder in a tab behind it, and a modal window would cover the thing you are judging.
/// </remarks>
public partial class DiskUsageWindow : ThemedWindow
{
    private readonly DiskUsageViewModel _vm;
    private readonly Action<string, bool> _reveal;

    /// <param name="reveal">Takes a path and whether it is a directory, and puts the app there —
    /// supplied rather than reached for, so this window knows nothing about the shell.</param>
    private readonly IThemeService _themes;

    public DiskUsageWindow(DiskUsageViewModel vm, Action<string, bool> reveal)
    {
        InitializeComponent();
        _vm = vm;
        _reveal = reveal;
        DataContext = vm;

        _themes = App.Services.GetRequiredService<IThemeService>();
        Treemap.ApplyTheme(_themes.Current);
        _themes.ThemeChanged += OnThemeChanged;

        Treemap.TileActivated += OnTileActivated;

        // The map is drawn from the same children the list binds to, so it follows every load and
        // every drill without either side having to know about the other.
        _vm.Children.CollectionChanged += (_, _) => Treemap.SetItems(_vm.Children);

        Closed += (_, _) =>
        {
            // The view model holds subscriptions to the index service, and this window holds one to
            // the theme service — both outlive it, and this window is modeless.
            _themes.ThemeChanged -= OnThemeChanged;
            _vm.Dispose();
        };
    }

    private void OnThemeChanged(object? sender, EventArgs e) =>
        Dispatcher.InvokeAsync(() => Treemap.ApplyTheme(_themes.Current));

    /// <summary>A tile behaves as its row does; the folded "smaller items" tile stands for many
    /// things and so opens none of them.</summary>
    private void OnTileActivated(DiskUsageTileViewModel? tile)
    {
        if (tile is null || tile.IsSynthetic) return;

        if (tile.IsDirectory)
            _ = _vm.LoadAsync(tile.FullPath);
        else
            _reveal(tile.FullPath, false);
    }

    /// <summary>The harness photographs this window without ever showing it, and goes through the
    /// same constructor so a capture cannot drift from what the app puts on screen.</summary>
    internal static DiskUsageWindow Create(DiskUsageViewModel vm, Action<string, bool> reveal) =>
        new(vm, reveal);

    /// <summary>Opens the window on <paramref name="path"/> (null being "This PC").</summary>
    public void Load(string? path) => _ = _vm.LoadAsync(path);

    /// <summary>A folder drills in; anything else is a leaf and goes to the app instead.</summary>
    private void Child_DoubleClick(object sender, RoutedEventArgs e)
    {
        if (((ListBox)sender).SelectedItem is not DiskUsageTileViewModel tile) return;

        // The unaccounted row stands for "everything not itemised here" and is not a path.
        if (tile.IsSynthetic) return;

        if (tile.IsDirectory)
            _ = _vm.LoadAsync(tile.FullPath);
        else
            _reveal(tile.FullPath, false);
    }

    private void File_DoubleClick(object sender, RoutedEventArgs e)
    {
        if (((ListBox)sender).SelectedItem is FileItemViewModel item)
            _reveal(item.FullPath, item.IsDirectory);
    }
}
