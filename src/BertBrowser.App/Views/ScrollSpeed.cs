using System.Windows;
using System.Windows.Input;
using BertBrowser.App.Services;

namespace BertBrowser.App.Views;

/// <summary>Applies the configurable scroll-speed multiplier to mouse-wheel scrolling. Shared by
/// the folder tree and by every pane's file list.</summary>
internal static class ScrollSpeed
{
    /// <summary>Reproduces WPF's default (WheelScrollLines lines per notch) scaled by the setting,
    /// so 1× matches the system and 2× (the default) is twice as fast.</summary>
    public static void HandlePreviewMouseWheel(object sender, MouseWheelEventArgs e, AppSettings settings)
    {
        // Let Shift+wheel do its native horizontal scroll untouched.
        if (e.Delta == 0 || e.Handled || Keyboard.Modifiers.HasFlag(ModifierKeys.Shift)) return;

        var scrollViewer = VisualTreeUtil.FindScrollViewer((DependencyObject)sender);
        if (scrollViewer is null) return;

        var wheelLines = SystemParameters.WheelScrollLines;
        if (wheelLines <= 0) wheelLines = 3; // -1 = "one page"; fall back to the common default
        var lines = (int)Math.Round(Math.Abs(e.Delta) / 120.0 * wheelLines * settings.ScrollSpeedMultiplier);
        lines = Math.Clamp(lines, 1, 240);

        for (var i = 0; i < lines; i++)
        {
            if (e.Delta > 0) scrollViewer.LineUp();
            else scrollViewer.LineDown();
        }
        e.Handled = true;
    }
}
