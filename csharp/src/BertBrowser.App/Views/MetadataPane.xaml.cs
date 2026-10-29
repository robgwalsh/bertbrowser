using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Threading;

namespace BertBrowser.App.Views;

/// <summary>
/// One tab's metadata pane: what the selection says about itself, in boxes that can be typed in.
/// All of it is <see cref="ViewModels.MetadataViewModel"/>; this is only the date popup's manners.
/// </summary>
public partial class MetadataPane : UserControl
{
    public MetadataPane() => InitializeComponent();

    private ViewModels.MetadataViewModel? Editor => DataContext as ViewModels.MetadataViewModel;

    /// <summary>The picker is the view's job; embedding what it picked is the view model's.</summary>
    private async void ChoosePicture_Click(object sender, RoutedEventArgs e)
    {
        if (Editor is not { } editor) return;

        var dialog = new Microsoft.Win32.OpenFileDialog
        {
            Title = "Choose a cover picture",
            Filter = "Pictures|*.jpg;*.jpeg;*.png|All files|*.*",
        };
        if (dialog.ShowDialog(Window.GetWindow(this)) != true) return;

        byte[] image;
        try
        {
            // A cover is embedded whole in every selected file; a picture of tens of megabytes
            // was chosen by mistake.
            if (new System.IO.FileInfo(dialog.FileName).Length > 16 << 20)
            {
                MessageDialog.Show(Window.GetWindow(this), "That picture is larger than 16 MB, which is too large to embed as a cover.",
                    "Cover picture", MessageDialogKind.Warning);
                return;
            }

            image = await System.IO.File.ReadAllBytesAsync(dialog.FileName);
        }
        catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException)
        {
            MessageDialog.Show(Window.GetWindow(this), $"The picture could not be read: {ex.Message}",
                "Cover picture", MessageDialogKind.Warning);
            return;
        }

        await editor.SetPictureAsync(image);
    }

    private void SavePicture_Click(object sender, RoutedEventArgs e)
    {
        if (Editor?.PictureBytes is not { } bytes) return;

        // Named for what the bytes are, which is the picture's business and not the song's.
        var png = bytes.AsSpan().StartsWith((ReadOnlySpan<byte>)[0x89, 0x50, 0x4E, 0x47]);
        var dialog = new Microsoft.Win32.SaveFileDialog
        {
            Title = "Save cover picture",
            FileName = png ? "cover.png" : "cover.jpg",
            Filter = png ? "PNG picture|*.png" : "JPEG picture|*.jpg",
        };
        if (dialog.ShowDialog(Window.GetWindow(this)) != true) return;

        try
        {
            System.IO.File.WriteAllBytes(dialog.FileName, bytes);
        }
        catch (Exception ex) when (ex is System.IO.IOException or UnauthorizedAccessException)
        {
            MessageDialog.Show(Window.GetWindow(this), $"The picture could not be saved: {ex.Message}",
                "Cover picture", MessageDialogKind.Warning);
        }
    }

    /// <summary>
    /// Closes the calendar once a day is clicked, and gives the mouse back.
    /// </summary>
    /// <remarks>
    /// On the click rather than on the selection changing, because the selection also changes when
    /// the popup opens and the binding hands the calendar the date already in the box — which
    /// would close it before it was seen. And a <see cref="Calendar"/> keeps mouse capture after a
    /// click on a day, so without the release the next click anywhere — Apply, most likely — is
    /// swallowed and has to be made twice. Posted, so the calendar finishes selecting first.
    /// </remarks>
    private void Calendar_PreviewMouseUp(object sender, MouseButtonEventArgs e)
    {
        if (sender is not Calendar { Parent: Popup popup }) return;
        if (e.OriginalSource is not DependencyObject source ||
            VisualTreeUtil.FindAncestor<CalendarDayButton>(source) is null)
            return;

        _ = Dispatcher.BeginInvoke(() =>
        {
            Mouse.Capture(null);
            popup.IsOpen = false;
        }, DispatcherPriority.Input);
    }
}
