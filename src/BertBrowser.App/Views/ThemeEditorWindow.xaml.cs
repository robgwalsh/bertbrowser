using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Data;
using BertBrowser.App.Services;
using BertBrowser.App.ViewModels;
using Microsoft.Win32;

namespace BertBrowser.App.Views;

public partial class ThemeEditorWindow : ThemedWindow
{
    private readonly AppearanceViewModel _vm;
    private Popup? _pickerPopup;

    public ThemeEditorWindow(AppearanceViewModel vm)
    {
        InitializeComponent();
        _vm = vm;
        DataContext = vm;
    }

    /// <summary>
    /// Opens the colour picker over the clicked swatch. One popup is reused rather than one per
    /// row: there are a hundred rows and only ever one picker open.
    /// </summary>
    private void Swatch_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: ThemeTokenViewModel token } button) return;

        _pickerPopup ??= new Popup
        {
            AllowsTransparency = true,
            StaysOpen = false,
            Placement = PlacementMode.Bottom,
            Child = new Border
            {
                Background = (System.Windows.Media.Brush)FindResource("Theme.Menu.Background"),
                BorderBrush = (System.Windows.Media.Brush)FindResource("Theme.Menu.Border"),
                BorderThickness = new Thickness(1),
                Child = new ColorPicker(),
            },
        };

        var picker = (ColorPicker)((Border)_pickerPopup.Child).Child;

        // Rebound per swatch so edits land on the row that was clicked; two-way, so dragging in the
        // picker recolours the app live through the token view model.
        BindingOperations.SetBinding(picker, ColorPicker.SelectedColorProperty, new Binding
        {
            Source = token,
            Path = new PropertyPath(nameof(ThemeTokenViewModel.Color)),
            Mode = BindingMode.TwoWay,
        });

        _pickerPopup.PlacementTarget = button;
        _pickerPopup.IsOpen = true;
    }

    private void SaveAs_Click(object sender, RoutedEventArgs e)
    {
        Directory.CreateDirectory(AppPaths.ThemesDir);
        var dialog = new SaveFileDialog
        {
            Title = "Save theme as",
            InitialDirectory = AppPaths.ThemesDir,
            FileName = "My theme.json",
            Filter = "Theme files (*.json)|*.json",
            DefaultExt = ".json",
        };
        if (dialog.ShowDialog(this) != true) return;

        // The file name doubles as the theme's display name, which avoids a second prompt.
        var name = Path.GetFileNameWithoutExtension(dialog.FileName);
        if (!_vm.TrySaveAsNewTheme(name, out var error))
            MessageDialog.Show(this, error ?? "The theme could not be saved.", "Save theme", MessageDialogKind.Warning);
    }

    private void Import_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Import a theme",
            Filter = "Theme files (*.json)|*.json|All files (*.*)|*.*",
        };
        if (dialog.ShowDialog(this) != true) return;

        if (!_vm.TryImport(dialog.FileName, out var error))
            MessageDialog.Show(this, error ?? "The theme could not be imported.", "Import theme", MessageDialogKind.Warning);
    }

    private void Export_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new SaveFileDialog
        {
            Title = "Export the current theme",
            FileName = $"{_vm.SelectedTheme?.Name ?? "theme"}.json",
            Filter = "Theme files (*.json)|*.json",
            DefaultExt = ".json",
        };
        if (dialog.ShowDialog(this) != true) return;

        if (!_vm.TryExport(dialog.FileName, out var error))
            MessageDialog.Show(this, error ?? "The theme could not be exported.", "Export theme", MessageDialogKind.Warning);
    }
}
