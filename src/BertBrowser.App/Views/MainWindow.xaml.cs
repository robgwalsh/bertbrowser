using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using BertBrowser.App.ViewModels;
using BertBrowser.Core.Services;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

public partial class MainWindow : Window
{
    private readonly ShellViewModel _shell;
    private readonly BertBrowser.App.Services.AppSettings _settings;

    public MainWindow(ShellViewModel shell, BertBrowser.App.Services.AppSettings settings)
    {
        InitializeComponent();
        _shell = shell;
        _settings = settings;
        DataContext = shell;

        ApplyWindowSettings();

        _shell.FileList.PropertyChanged += FileList_PropertyChanged;
        UpdateRelPathColumn();

        Loaded += async (_, _) => await _shell.InitializeAsync();
        Closing += (_, _) => SaveWindowSettings();
    }

    private void ApplyWindowSettings()
    {
        if (_settings is { WindowWidth: > 200, WindowHeight: > 150 })
        {
            Width = _settings.WindowWidth!.Value;
            Height = _settings.WindowHeight!.Value;
        }
        if (_settings is { WindowLeft: { } left, WindowTop: { } top } &&
            left > SystemParameters.VirtualScreenLeft - 100 &&
            top > SystemParameters.VirtualScreenTop - 100 &&
            left < SystemParameters.VirtualScreenLeft + SystemParameters.VirtualScreenWidth - 100 &&
            top < SystemParameters.VirtualScreenTop + SystemParameters.VirtualScreenHeight - 100)
        {
            WindowStartupLocation = WindowStartupLocation.Manual;
            Left = left;
            Top = top;
        }
        if (_settings.WindowMaximized)
            WindowState = WindowState.Maximized;
    }

    private void SaveWindowSettings()
    {
        var bounds = WindowState == WindowState.Normal
            ? new Rect(Left, Top, Width, Height)
            : RestoreBounds;
        _settings.WindowLeft = bounds.Left;
        _settings.WindowTop = bounds.Top;
        _settings.WindowWidth = bounds.Width;
        _settings.WindowHeight = bounds.Height;
        _settings.WindowMaximized = WindowState == WindowState.Maximized;
        _settings.LastPath = _shell.CurrentPath.Length > 0 ? _shell.CurrentPath : null;
        _settings.Save();
    }

    private void FileList_PropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(FileListViewModel.IsFlattened))
            UpdateRelPathColumn();
    }

    /// <summary>The Relative path column only makes sense in flattened tag-filter mode.</summary>
    private void UpdateRelPathColumn() =>
        RelPathColumn.Width = _shell.FileList.IsFlattened ? 220 : 0;

    // --- Toolbar / dialogs ---

    private async void ManageTags_Click(object sender, RoutedEventArgs e)
    {
        var vm = new TagManagerViewModel(App.Services.GetRequiredService<ITagService>());
        var window = new TagManagerWindow(vm) { Owner = this };
        window.ShowDialog();
        await _shell.OnTagsChangedAsync();
    }

    // --- Breadcrumb ---

    private void BreadcrumbSegment_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string path })
            _ = _shell.NavigateToAsync(path);
        e.Handled = true;
    }

    private void Breadcrumb_EmptyClick(object sender, MouseButtonEventArgs e)
    {
        PathBox.Text = _shell.CurrentPath;
        Breadcrumb.Visibility = Visibility.Collapsed;
        PathBox.Visibility = Visibility.Visible;
        PathBox.Focus();
        PathBox.SelectAll();
    }

    private void PathBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.Enter)
        {
            var path = PathBox.Text.Trim();
            HidePathBox();
            if (path.Length > 0)
                _ = _shell.NavigateToAsync(path);
        }
        else if (e.Key == Key.Escape)
        {
            HidePathBox();
        }
    }

    private void PathBox_LostFocus(object sender, RoutedEventArgs e) => HidePathBox();

    private void HidePathBox()
    {
        PathBox.Visibility = Visibility.Collapsed;
        Breadcrumb.Visibility = Visibility.Visible;
    }

    // --- File list interactions ---

    private void FileList_HeaderClick(object sender, RoutedEventArgs e)
    {
        if (e.OriginalSource is GridViewColumnHeader { Tag: string tag } &&
            Enum.TryParse<SortColumn>(tag, out var column))
        {
            _shell.FileList.SetSort(column);
        }
    }

    private void FileList_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenItemCommand.Execute(item);
    }

    private void ContextOpen_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.OpenItemCommand.Execute(item);
    }

    private async void ContextTags_Click(object sender, RoutedEventArgs e)
    {
        var paths = FileListView.SelectedItems.Cast<FileItemViewModel>()
            .Where(i => !i.IsDirectory)
            .Select(i => i.FullPath)
            .ToList();
        if (paths.Count == 0)
        {
            _shell.StatusText = "Select one or more files to tag (folders cannot be tagged).";
            return;
        }

        var vm = new AssignTagsViewModel(App.Services.GetRequiredService<ITagService>(), paths);
        var dialog = new AssignTagsDialog(vm) { Owner = this };
        if (dialog.ShowDialog() == true)
            await _shell.OnTagsChangedAsync();
    }

    private void ContextComputeSize_Click(object sender, RoutedEventArgs e)
    {
        var items = FileListView.SelectedItems.Cast<FileItemViewModel>().ToList();
        _shell.ComputeSizeCommand.Execute(items);
    }

    private void ContextRemoveMissing_Click(object sender, RoutedEventArgs e)
    {
        if (FileListView.SelectedItem is FileItemViewModel item)
            _shell.RemoveMissingCommand.Execute(item);
    }
}
