using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Shell;
using BertBrowser.App.Theming;
using BertBrowser.Core.Theming;
using Microsoft.Extensions.DependencyInjection;

namespace BertBrowser.App.Views;

/// <summary>
/// A window with a title bar we draw ourselves, so the theme reaches the very edge of the app
/// instead of stopping below a Windows-coloured caption.
/// </summary>
/// <remarks>
/// <para>
/// This is a <see cref="Window"/> subclass rather than just a style because three of the things a
/// custom caption has to get right need the window handle: clamping a maximised window to the
/// monitor's work area, answering the hit test that makes Windows 11 offer snap layouts, and
/// telling DWM to darken the frame it still draws.
/// </para>
/// <para>
/// Note that an implicit <c>Style TargetType="Window"</c> would not reach any of the app's windows —
/// implicit styles key on the exact type — which is the other reason the shared chrome lives on a
/// common base class instead. That same rule bites this class too, which is why the constructor asks
/// for the style by name; see there.
/// </para>
/// </remarks>
public class ThemedWindow : Window
{
    private const int WmGetMinMaxInfo = 0x0024;
    private const int WmNcHitTest = 0x0084;
    private const int WmNcLButtonDown = 0x00A1;
    private const int WmNcLButtonUp = 0x00A2;
    private const int WmNcMouseLeave = 0x02A2;
    private const int WmDpiChanged = 0x02E0;

    private const int HtMaxButton = 9;
    private const int MonitorDefaultToNearest = 2;

    /// <summary>
    /// True while the pointer is over the maximise button. Tracked by hand because the button is
    /// reported as non-client for hit-testing (that is what enables the snap-layout flyout), and WPF
    /// stops raising mouse events on it as a result.
    /// </summary>
    public static readonly DependencyProperty IsMaximizeHoveredProperty = DependencyProperty.Register(
        nameof(IsMaximizeHovered), typeof(bool), typeof(ThemedWindow), new PropertyMetadata(false));

    /// <summary>Optional content hosted in the title bar, beside the window title.</summary>
    public static readonly DependencyProperty TitleBarContentProperty = DependencyProperty.Register(
        nameof(TitleBarContent), typeof(object), typeof(ThemedWindow), new PropertyMetadata(null));

    private IThemeService? _theme;
    private Button? _maximizeButton;

    public ThemedWindow()
    {
        // WPF looks an implicit style up under the element's *exact* runtime type and never walks
        // the base chain, and no window here is a ThemedWindow — they are all subclasses of one. So
        // `Style TargetType="{x:Type v:ThemedWindow}"` reaches nothing on its own and every window
        // silently falls back to the stock Window template: native caption, and TitleBarContent
        // (which is where MainWindow's toolbar lives) dropped on the floor. Asking for the style by
        // that key explicitly is what makes the subclasses pick it up. A local value, so a window
        // that sets Style in its own XAML still wins.
        SetResourceReference(StyleProperty, typeof(ThemedWindow));

        // Resolved rather than injected: dialogs are constructed with `new` from static prompt
        // helpers, following the same service-locator route PropertiesPrompt already uses.
        _theme = App.Services?.GetService<IThemeService>();
        if (_theme is not null) _theme.ThemeChanged += OnThemeChanged;
        Closed += (_, _) =>
        {
            if (_theme is not null) _theme.ThemeChanged -= OnThemeChanged;
            _theme = null;
        };
    }

    public bool IsMaximizeHovered
    {
        get => (bool)GetValue(IsMaximizeHoveredProperty);
        set => SetValue(IsMaximizeHoveredProperty, value);
    }

    public object? TitleBarContent
    {
        get => GetValue(TitleBarContentProperty);
        set => SetValue(TitleBarContentProperty, value);
    }

    public override void OnApplyTemplate()
    {
        base.OnApplyTemplate();

        _maximizeButton = GetTemplateChild("PART_Maximize") as Button;

        Wire("PART_Minimize", () => WindowState = WindowState.Minimized);
        Wire("PART_Maximize", ToggleMaximized);
        Wire("PART_Close", Close);

        // A dialog that says it cannot be resized must not become resizable just because we took
        // over the frame — WindowChrome's resize border does not consult ResizeMode.
        //
        // The chrome must be replaced, not adjusted: it arrives from a Setter in the shared style,
        // which means it is both sealed — assigning to it throws "in a read-only state", and the
        // window then fails to open at all — and shared, so a successful assignment would have
        // taken the resize border off every other window too. A clone set locally on this window
        // beats the setter and belongs to nobody else.
        if (ResizeMode is ResizeMode.NoResize or ResizeMode.CanMinimize &&
            WindowChrome.GetWindowChrome(this) is { } chrome &&
            chrome.ResizeBorderThickness != default)
        {
            var own = (WindowChrome)chrome.Clone();
            own.ResizeBorderThickness = default;
            WindowChrome.SetWindowChrome(this, own);
        }
    }

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);

        if (PresentationSource.FromVisual(this) is HwndSource source)
        {
            source.AddHook(WndProc);
        }

        ApplyFrameColours();
    }

    private void Wire(string partName, Action action)
    {
        if (GetTemplateChild(partName) is Button button) button.Click += (_, _) => action();
    }

    private void ToggleMaximized() =>
        WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;

    private void OnThemeChanged(object? sender, EventArgs e) => ApplyFrameColours();

    private void ApplyFrameColours()
    {
        if (_theme is null) return;
        var handle = new WindowInteropHelper(this).Handle;
        DwmInterop.Apply(
            handle,
            _theme.Current.IsDark,
            ThemeTokenDictionary.ToMediaColor(_theme.Current[ThemeToken.TitleBarBorder]),
            ThemeTokenDictionary.ToMediaColor(_theme.Current[ThemeToken.TitleBarBackground]),
            ThemeTokenDictionary.ToMediaColor(_theme.Current[ThemeToken.TitleBarForeground]));
    }

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        switch (msg)
        {
            case WmGetMinMaxInfo:
                ClampToWorkArea(hwnd, lParam);
                break;

            case WmNcHitTest:
                if (IsOverMaximizeButton(lParam))
                {
                    IsMaximizeHovered = true;
                    handled = true;
                    return HtMaxButton;
                }
                IsMaximizeHovered = false;
                break;

            // With the button reported as HTMAXBUTTON, Windows sends non-client clicks for it.
            // Swallowing the down stops a caption drag from starting on the button.
            case WmNcLButtonDown when wParam.ToInt32() == HtMaxButton:
                handled = true;
                break;

            case WmNcLButtonUp when wParam.ToInt32() == HtMaxButton:
                ToggleMaximized();
                handled = true;
                break;

            case WmNcMouseLeave:
                IsMaximizeHovered = false;
                break;

            case WmDpiChanged:
                // The clamp is computed in physical pixels, so it has to be redone for the new
                // scale — otherwise a maximised window dragged to a differently-scaled monitor
                // keeps the old monitor's metrics.
                ApplyFrameColours();
                break;
        }

        return IntPtr.Zero;
    }

    /// <summary>
    /// Without this, a maximised window under <see cref="WindowChrome"/> is sized to the whole
    /// monitor including the invisible resize frame: the edges are clipped and the taskbar is
    /// covered. Fixing it with a fixed margin instead is tempting and wrong — the frame is
    /// DPI-dependent.
    /// </summary>
    private static void ClampToWorkArea(IntPtr hwnd, IntPtr lParam)
    {
        var monitor = MonitorFromWindow(hwnd, MonitorDefaultToNearest);
        if (monitor == IntPtr.Zero) return;

        var info = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
        if (!GetMonitorInfo(monitor, ref info)) return;

        var minMax = Marshal.PtrToStructure<MinMaxInfo>(lParam);
        minMax.MaxPosition.X = info.Work.Left - info.Monitor.Left;
        minMax.MaxPosition.Y = info.Work.Top - info.Monitor.Top;
        minMax.MaxSize.X = info.Work.Right - info.Work.Left;
        minMax.MaxSize.Y = info.Work.Bottom - info.Work.Top;
        minMax.MaxTrackSize = minMax.MaxSize;
        Marshal.StructureToPtr(minMax, lParam, true);
    }

    private bool IsOverMaximizeButton(IntPtr lParam)
    {
        if (_maximizeButton is not { IsVisible: true } button) return false;

        // lParam packs a screen point as two signed 16-bit values.
        var x = (short)(lParam.ToInt32() & 0xFFFF);
        var y = (short)((lParam.ToInt32() >> 16) & 0xFFFF);

        try
        {
            var local = button.PointFromScreen(new Point(x, y));
            return new Rect(default, button.RenderSize).Contains(local);
        }
        catch (InvalidOperationException)
        {
            // The button is not connected to a presentation source yet.
            return false;
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativePoint
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MinMaxInfo
    {
        public NativePoint Reserved;
        public NativePoint MaxSize;
        public NativePoint MaxPosition;
        public NativePoint MinTrackSize;
        public NativePoint MaxTrackSize;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MonitorInfo
    {
        public int Size;
        public NativeRect Monitor;
        public NativeRect Work;
        public uint Flags;
    }

    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern IntPtr MonitorFromWindow(IntPtr hwnd, int flags);

    [DllImport("user32.dll", EntryPoint = "GetMonitorInfoW", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);
}
