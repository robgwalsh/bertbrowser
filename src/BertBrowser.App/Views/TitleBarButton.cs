using System.Windows;
using System.Windows.Controls;

namespace BertBrowser.App.Views;

/// <summary>
/// The minimise/maximise/close buttons in <see cref="ThemedWindow"/>'s title bar. WPF's own modal
/// dialogs work by setting the owner <see cref="Window"/>'s <c>IsEnabled</c> to false, which
/// cascades down and would take these buttons out with the rest of the content — but the title bar
/// is chrome we draw ourselves, and a modal child shouldn't strand the owner: you should still be
/// able to drag, minimise, or close it. The coercion below pins IsEnabled to true regardless of
/// what the Window says; <see cref="ThemedWindow"/>'s WM_ENABLE handling does the matching job at
/// the Win32 level so non-client dragging survives too.
/// </summary>
public sealed class TitleBarButton : Button
{
    static TitleBarButton()
    {
        IsEnabledProperty.OverrideMetadata(typeof(TitleBarButton),
            new UIPropertyMetadata(true, null, (_, _) => true));
    }
}
